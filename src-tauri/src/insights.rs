//! Incremental, local-first Voice Insights projection.
//!
//! History remains the source of truth. This module stores reversible per-entry
//! contributions plus daily buckets, so opening Insights never scans the full
//! transcription history or re-reads every audio file.

use crate::models::{AppState, HistoryEntry};
use crate::pipeline_run::{epoch_ms, StageKind};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, OnceLock};
use std::time::Duration;
use tauri::Emitter;

pub const INSIGHTS_SCHEMA_VERSION: u32 = 4;
pub const ANALYSIS_VERSION: u32 = 1;
pub const VOICE_PROFILE_MODEL: &str = "google/gemini-3.7-flash";
pub const VOICE_PROFILE_FALLBACK_MODEL: &str = "meta/muse-spark-1.2-contributor";
const PROFILE_MIN_WORDS: u64 = 5_000;
const PROFILE_REFRESH_WORDS: u64 = 10_000;
const MIN_TREND_SESSIONS: u64 = 3;

static STORE_PATH: OnceLock<PathBuf> = OnceLock::new();
static BUCKET_DIR: OnceLock<PathBuf> = OnceLock::new();
static STORE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static JOB_SENDER: OnceLock<mpsc::Sender<InsightJob>> = OnceLock::new();
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();
static BACKFILL_PAUSED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone)]
enum InsightJob {
    Upsert(Box<HistoryEntry>),
    Remove(String),
    Clear,
}

#[derive(Debug)]
enum PreparedInsightJob {
    Upsert(Box<InsightContribution>),
    Remove(String),
    Clear,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsightPeriod {
    Today,
    Last7Days,
    #[default]
    Last30Days,
    AllTime,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackfillStatus {
    pub running: bool,
    pub paused: bool,
    pub processed: u64,
    pub total: u64,
    pub analyzed: u64,
    pub unavailable_audio: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AudioMetrics {
    pub duration_ms: u64,
    pub peak_dbfs: f64,
    pub rms_dbfs: f64,
    /// Integrated loudness estimate for mono speech. UI always labels this as estimated.
    pub lufs: f64,
    pub clipping_ratio: f64,
    pub silence_ratio: f64,
    pub speech_duration_ms: u64,
    pub noise_floor_estimate_dbfs: f64,
    pub snr_estimate_db: f64,
    pub pause_count: u32,
    pub mean_pause_duration_ms: Option<f64>,
    pub median_pause_duration_ms: Option<f64>,
    pub f0_mean_hz: Option<f64>,
    pub f0_median_hz: Option<f64>,
    pub f0_stddev_hz: Option<f64>,
    pub pitch_range_hz: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct InsightContribution {
    id: String,
    analysis_version: u32,
    day: String,
    hour: u8,
    weekday: u8,
    words: u64,
    duration_ms: u64,
    manual_corrections: u64,
    self_corrections: u64,
    app: Option<String>,
    domain: Option<String>,
    category: String,
    language: String,
    word_counts: BTreeMap<String, u64>,
    content_word_counts: BTreeMap<String, u64>,
    phrase_counts: BTreeMap<String, u64>,
    phrase_sessions: BTreeSet<String>,
    filler_counts: BTreeMap<String, u64>,
    mattr: Option<f64>,
    wpm: Option<f64>,
    audio: Option<AudioMetrics>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ContributionMarker {
    analysis_version: u32,
    day: String,
    audio_analyzed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AggregateCore {
    sessions: u64,
    words: u64,
    duration_ms: u64,
    manual_corrections: u64,
    self_corrections: u64,
    word_counts: BTreeMap<String, u64>,
    content_word_counts: BTreeMap<String, u64>,
    phrase_counts: BTreeMap<String, u64>,
    phrase_sessions: BTreeMap<String, u64>,
    filler_counts: BTreeMap<String, u64>,
    apps: BTreeMap<String, u64>,
    domains: BTreeMap<String, u64>,
    app_domains: BTreeMap<String, u64>,
    categories: BTreeMap<String, u64>,
    hours: BTreeMap<u8, u64>,
    weekdays: BTreeMap<u8, u64>,
    language_counts: BTreeMap<String, u64>,
    mattr_weighted_sum: f64,
    mattr_weight: u64,
    wpm_samples: Vec<f64>,
    audio_samples: Vec<AudioMetrics>,
}

impl AggregateCore {
    fn add_contribution(&mut self, contribution: &InsightContribution) {
        self.sessions += 1;
        self.words += contribution.words;
        self.duration_ms += contribution.duration_ms;
        self.manual_corrections += contribution.manual_corrections;
        self.self_corrections += contribution.self_corrections;
        merge_counts(&mut self.word_counts, &contribution.word_counts);
        merge_counts(
            &mut self.content_word_counts,
            &contribution.content_word_counts,
        );
        merge_counts(&mut self.phrase_counts, &contribution.phrase_counts);
        for phrase in &contribution.phrase_sessions {
            *self.phrase_sessions.entry(phrase.clone()).or_default() += 1;
        }
        merge_counts(&mut self.filler_counts, &contribution.filler_counts);
        if let Some(app) = contribution.app.as_ref() {
            *self.apps.entry(app.clone()).or_default() += 1;
        }
        if let Some(domain) = contribution.domain.as_ref() {
            *self.domains.entry(domain.clone()).or_default() += 1;
            if let Some(app) = contribution.app.as_ref() {
                *self
                    .app_domains
                    .entry(format!("{app}\u{1f}{domain}"))
                    .or_default() += 1;
            }
        }
        *self
            .categories
            .entry(contribution.category.clone())
            .or_default() += 1;
        *self.hours.entry(contribution.hour).or_default() += 1;
        *self.weekdays.entry(contribution.weekday).or_default() += 1;
        *self
            .language_counts
            .entry(contribution.language.clone())
            .or_default() += 1;
        if let Some(mattr) = contribution.mattr {
            self.mattr_weighted_sum += mattr * contribution.words.max(1) as f64;
            self.mattr_weight += contribution.words.max(1);
        }
        if let Some(wpm) = contribution.wpm {
            self.wpm_samples.push(wpm);
        }
        if let Some(audio) = contribution.audio.as_ref() {
            self.audio_samples.push(audio.clone());
        }
    }

    fn merge(&mut self, other: &AggregateCore) {
        self.sessions += other.sessions;
        self.words += other.words;
        self.duration_ms += other.duration_ms;
        self.manual_corrections += other.manual_corrections;
        self.self_corrections += other.self_corrections;
        merge_counts(&mut self.word_counts, &other.word_counts);
        merge_counts(&mut self.content_word_counts, &other.content_word_counts);
        merge_counts(&mut self.phrase_counts, &other.phrase_counts);
        merge_counts(&mut self.phrase_sessions, &other.phrase_sessions);
        merge_counts(&mut self.filler_counts, &other.filler_counts);
        merge_counts(&mut self.apps, &other.apps);
        merge_counts(&mut self.domains, &other.domains);
        merge_counts(&mut self.app_domains, &other.app_domains);
        merge_counts(&mut self.categories, &other.categories);
        merge_counts(&mut self.hours, &other.hours);
        merge_counts(&mut self.weekdays, &other.weekdays);
        merge_counts(&mut self.language_counts, &other.language_counts);
        self.mattr_weighted_sum += other.mattr_weighted_sum;
        self.mattr_weight += other.mattr_weight;
        self.wpm_samples.extend_from_slice(&other.wpm_samples);
        self.audio_samples.extend_from_slice(&other.audio_samples);
    }
}

fn merge_counts<K: Ord + Clone>(target: &mut BTreeMap<K, u64>, source: &BTreeMap<K, u64>) {
    for (key, count) in source {
        *target.entry(key.clone()).or_default() += count;
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DailyBucketIndex {
    sessions: u64,
    words: u64,
    audio_sessions: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoiceProfile {
    pub title: String,
    pub description: String,
    pub generated_at_ms: u64,
    pub generated_at_word_count: u64,
    pub next_update_word_count: u64,
    pub profile_version: u32,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub request_ms: u64,
    #[serde(default)]
    pub ttfb_ms: Option<u64>,
    #[serde(default)]
    pub reported_total_tokens: Option<usize>,
    #[serde(default)]
    pub reported_input_tokens: Option<usize>,
    #[serde(default)]
    pub reported_output_tokens: Option<usize>,
    /// Provider-reported actual cost. `None` means unknown; no estimate is invented.
    #[serde(default)]
    pub reported_cost_usd: Option<f64>,
    #[serde(default)]
    pub generation_id: Option<String>,
    #[serde(default)]
    pub bytes_sent: u64,
    #[serde(default)]
    pub attempts: Vec<VoiceProfileAttempt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceProfileAttempt {
    pub provider: String,
    pub model: String,
    pub status: String,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InsightsStore {
    schema_version: u32,
    analysis_version: u32,
    contributions: BTreeMap<String, ContributionMarker>,
    daily: BTreeMap<String, DailyBucketIndex>,
    backfill: BackfillStatus,
    ai_profile_enabled: bool,
    profile: Option<VoiceProfile>,
    updated_at_ms: u64,
    #[cfg(test)]
    #[serde(skip)]
    test_contributions: BTreeMap<String, InsightContribution>,
}

impl Default for InsightsStore {
    fn default() -> Self {
        Self {
            schema_version: INSIGHTS_SCHEMA_VERSION,
            analysis_version: ANALYSIS_VERSION,
            contributions: BTreeMap::new(),
            daily: BTreeMap::new(),
            backfill: BackfillStatus::default(),
            ai_profile_enabled: false,
            profile: None,
            updated_at_ms: epoch_ms(),
            #[cfg(test)]
            test_contributions: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedCount {
    pub label: String,
    pub count: u64,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillerInsight {
    pub phrase: String,
    pub count: u64,
    pub per_1000_words: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationInsight {
    pub name: String,
    pub count: u64,
    pub percentage: f64,
    pub domains: Vec<RankedCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionInsight {
    pub before: String,
    pub after: String,
    pub count: u64,
    pub in_vocabulary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInsights {
    pub sessions: u64,
    pub words: u64,
    pub audio_duration_ms: u64,
    pub average_wpm: Option<f64>,
    pub median_wpm: Option<f64>,
    pub typical_wpm: Option<[f64; 2]>,
    pub manual_corrections: u64,
    pub vocabulary_corrections: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageInsights {
    pub language: String,
    pub most_used_word: Option<RankedCount>,
    pub most_used_content_word: Option<RankedCount>,
    pub most_used_phrase: Option<RankedCount>,
    pub catchphrase: Option<RankedCount>,
    pub fillers: Vec<FillerInsight>,
    pub self_corrections_per_1000_words: Option<f64>,
    pub vocabulary_variety: Option<f64>,
    pub vocabulary_variety_label: Option<String>,
    pub most_corrected: Option<CorrectionInsight>,
    pub catchphrase_ready: bool,
    pub profile_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioInsights {
    pub analyzed_sessions: u64,
    pub coverage_percentage: f64,
    pub lufs_median: Option<f64>,
    pub lufs_typical: Option<[f64; 2]>,
    pub rms_dbfs_median: Option<f64>,
    pub peak_dbfs_median: Option<f64>,
    pub clipping_ratio: Option<f64>,
    pub silence_ratio: Option<f64>,
    pub speech_ratio: Option<f64>,
    pub estimated_snr_db: Option<f64>,
    pub average_pause_ms: Option<f64>,
    pub median_f0_hz: Option<f64>,
    pub pitch_variation: Option<String>,
    pub capture_quality: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalInsights {
    pub peak_hour: Option<u8>,
    pub peak_weekday: Option<u8>,
    pub current_streak_days: u64,
    pub longest_streak_days: u64,
    pub activity: Vec<DailyActivity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyActivity {
    pub day: String,
    pub sessions: u64,
    pub words: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricTrend {
    pub metric: String,
    pub current: f64,
    pub previous: f64,
    pub change_percent: Option<f64>,
    pub change_absolute: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightsResponse {
    pub analysis_version: u32,
    pub period: InsightPeriod,
    pub usage: UsageInsights,
    pub language: LanguageInsights,
    pub audio: AudioInsights,
    pub applications: Vec<RankedCount>,
    pub application_details: Vec<ApplicationInsight>,
    pub domains: Vec<RankedCount>,
    pub categories: Vec<RankedCount>,
    pub temporal: TemporalInsights,
    pub trends: Vec<MetricTrend>,
    pub profile_enabled: bool,
    pub profile: Option<VoiceProfile>,
    pub profile_progress_words: u64,
    pub profile_required_words: u64,
    pub backfill: BackfillStatus,
    pub generated_at_ms: u64,
}

pub fn init(data_dir: PathBuf) {
    let _ = STORE_PATH.set(data_dir.join("voice-insights-v1.json"));
    let _ = BUCKET_DIR.set(data_dir.join("voice-insights-buckets-v4"));
    let _ = STORE_LOCK.set(Mutex::new(()));
    let (sender, receiver) = mpsc::channel();
    let _ = JOB_SENDER.set(sender);
    std::thread::Builder::new()
        .name("haumea-insights".into())
        .spawn(move || insight_worker(receiver))
        .ok();

    // Migration/reanalysis is intentionally cheap on startup: only metadata is
    // read. Historical entries and audio are handled later by the backfill worker.
    let _guard = lock().lock();
    let mut store = read_store_unlocked();
    if store.schema_version != INSIGHTS_SCHEMA_VERSION || store.analysis_version != ANALYSIS_VERSION
    {
        let enabled = store.ai_profile_enabled;
        let profile = store.profile;
        store = InsightsStore::default();
        store.ai_profile_enabled = enabled;
        store.profile = profile;
        store.backfill.last_error = Some("analysis_version_changed".into());
        let _ = write_store_unlocked(&store);
    }
}

fn lock() -> &'static Mutex<()> {
    STORE_LOCK.get_or_init(|| Mutex::new(()))
}

fn read_store_unlocked() -> InsightsStore {
    STORE_PATH
        .get()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn write_store_unlocked(store: &InsightsStore) -> Result<(), String> {
    let path = STORE_PATH
        .get()
        .ok_or_else(|| "insights store not initialized".to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temp = path.with_extension("json.tmp");
    let json = serde_json::to_vec(store).map_err(|error| error.to_string())?;
    fs::write(&temp, json).map_err(|error| error.to_string())?;
    fs::rename(&temp, path).map_err(|error| error.to_string())
}

fn insight_worker(receiver: mpsc::Receiver<InsightJob>) {
    while let Ok(first) = receiver.recv() {
        let mut jobs = vec![first];
        while jobs.len() < 24 {
            match receiver.try_recv() {
                Ok(job) => jobs.push(job),
                Err(_) => break,
            }
        }
        // Audio and linguistic analysis may be expensive. Prepare the batch
        // without the store lock so snapshots remain immediately readable.
        let jobs: Vec<_> = jobs
            .into_iter()
            .map(|job| match job {
                InsightJob::Upsert(entry) => {
                    PreparedInsightJob::Upsert(Box::new(analyze_entry(&entry)))
                }
                InsightJob::Remove(id) => PreparedInsightJob::Remove(id),
                InsightJob::Clear => PreparedInsightJob::Clear,
            })
            .collect();
        let _guard = lock().lock();
        let mut store = read_store_unlocked();
        for job in jobs {
            let result = match job {
                PreparedInsightJob::Upsert(contribution) => {
                    upsert_contribution_unlocked(&mut store, *contribution)
                }
                PreparedInsightJob::Remove(id) => remove_entry_unlocked(&mut store, &id),
                PreparedInsightJob::Clear => {
                    let _ = clear_day_buckets();
                    let enabled = store.ai_profile_enabled;
                    store = InsightsStore::default();
                    store.ai_profile_enabled = enabled;
                    Ok(())
                }
            };
            if let Err(error) = result {
                log::warn!("insights: failed to apply incremental job: {error}");
            }
        }
        store.updated_at_ms = epoch_ms();
        if let Err(error) = write_store_unlocked(&store) {
            log::warn!("insights: failed to persist worker batch: {error}");
        } else if let Some(app) = APP_HANDLE.get() {
            let _ = app.emit("insights-updated", ());
        }
    }
}

pub fn enqueue_entry(entry: HistoryEntry) {
    if entry.is_error.unwrap_or(false) || entry.text.trim().is_empty() {
        return;
    }
    if let Some(sender) = JOB_SENDER.get() {
        let _ = sender.send(InsightJob::Upsert(Box::new(entry)));
    }
}

pub fn enqueue_remove(id: impl Into<String>) {
    if let Some(sender) = JOB_SENDER.get() {
        let _ = sender.send(InsightJob::Remove(id.into()));
    }
}

pub fn enqueue_clear() {
    if let Some(sender) = JOB_SENDER.get() {
        let _ = sender.send(InsightJob::Clear);
    }
}

pub fn start_backfill(app: tauri::AppHandle) {
    let _ = APP_HANDLE.set(app.clone());
    std::thread::Builder::new()
        .name("haumea-insights-backfill".into())
        .spawn(move || {
            let entries = crate::history::load_all();
            let valid: Vec<_> = entries
                .into_iter()
                .filter(|entry| !entry.is_error.unwrap_or(false) && !entry.text.trim().is_empty())
                .collect();
            let valid_ids: BTreeSet<_> = valid.iter().map(|entry| entry.id.clone()).collect();
            let (current_ids, unavailable_audio) = {
                let _guard = lock().lock();
                let mut store = read_store_unlocked();
                let stale_days: BTreeSet<_> = store
                    .contributions
                    .iter()
                    .filter(|(id, _)| !valid_ids.contains(*id))
                    .map(|(_, item)| item.day.clone())
                    .collect();
                store.contributions.retain(|id, _| valid_ids.contains(id));
                for day in stale_days {
                    let mut bucket = read_day_contributions(&store, &day);
                    bucket.retain(|id, _| valid_ids.contains(id));
                    let _ = write_day_contributions(&mut store, &day, &bucket);
                }
                let current_ids = current_contribution_ids(&store, &valid);
                let unavailable = valid
                    .iter()
                    .filter(|entry| {
                        store
                            .contributions
                            .get(&entry.id)
                            .is_some_and(|item| !item.audio_analyzed)
                    })
                    .count() as u64;
                store.backfill.running = true;
                store.backfill.paused = false;
                store.backfill.total = valid.len() as u64;
                store.backfill.processed = current_ids.len() as u64;
                store.backfill.analyzed = 0;
                store.backfill.unavailable_audio = unavailable;
                let _ = write_store_unlocked(&store);
                (current_ids, unavailable)
            };
            let already_processed = current_ids.len() as u64;
            let pending: Vec<_> = valid
                .into_iter()
                .filter(|entry| !current_ids.contains(&entry.id))
                .collect();
            let mut unavailable_audio = unavailable_audio;
            for (index, entry) in pending.iter().enumerate() {
                while BACKFILL_PAUSED.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(120));
                }
                let contribution = analyze_entry(entry);
                let audio_unavailable = contribution.audio.is_none();
                let _guard = lock().lock();
                let mut store = read_store_unlocked();
                if let Err(error) = upsert_contribution_unlocked(&mut store, contribution) {
                    store.backfill.last_error = Some(error);
                    let _ = write_store_unlocked(&store);
                    continue;
                }
                store.backfill.analyzed += 1;
                if audio_unavailable {
                    unavailable_audio += 1;
                }
                store.backfill.unavailable_audio = unavailable_audio;
                store.backfill.processed = already_processed + index as u64 + 1;
                store.updated_at_ms = epoch_ms();
                let _ = write_store_unlocked(&store);
                drop(_guard);
                if index % 8 == 0 {
                    let status = backfill_status();
                    let _ = app.emit("insights-progress", status);
                }
                std::thread::sleep(Duration::from_millis(4));
            }
            {
                let _guard = lock().lock();
                let mut store = read_store_unlocked();
                store.backfill.running = false;
                store.backfill.paused = false;
                store.backfill.processed = store.backfill.total;
                store.backfill.last_error = None;
                let _ = write_store_unlocked(&store);
            }
            let _ = app.emit("insights-progress", backfill_status());
        })
        .ok();
}

fn contribution_is_current(store: &InsightsStore, id: &str) -> bool {
    store
        .contributions
        .get(id)
        .is_some_and(|item| item.analysis_version == ANALYSIS_VERSION)
}

fn current_contribution_ids(store: &InsightsStore, entries: &[HistoryEntry]) -> BTreeSet<String> {
    entries
        .iter()
        .filter(|entry| contribution_is_current(store, &entry.id))
        .map(|entry| entry.id.clone())
        .collect()
}

pub fn set_backfill_paused(paused: bool) -> BackfillStatus {
    BACKFILL_PAUSED.store(paused, Ordering::Relaxed);
    let _guard = lock().lock();
    let mut store = read_store_unlocked();
    store.backfill.paused = paused;
    let status = store.backfill.clone();
    let _ = write_store_unlocked(&store);
    status
}

pub fn backfill_status() -> BackfillStatus {
    let _guard = lock().lock();
    read_store_unlocked().backfill
}

#[cfg(test)]
fn upsert_entry_unlocked(store: &mut InsightsStore, entry: &HistoryEntry) {
    let contribution = analyze_entry(entry);
    upsert_contribution_unlocked(store, contribution).unwrap();
}

fn upsert_contribution_unlocked(
    store: &mut InsightsStore,
    contribution: InsightContribution,
) -> Result<(), String> {
    let previous = store.contributions.get(&contribution.id).cloned();
    if let Some(previous) = previous
        .as_ref()
        .filter(|item| item.day != contribution.day)
    {
        let mut old_bucket = read_day_contributions(store, &previous.day);
        old_bucket.remove(&contribution.id);
        write_day_contributions(store, &previous.day, &old_bucket)?;
    }
    let mut bucket = read_day_contributions(store, &contribution.day);
    bucket.insert(contribution.id.clone(), contribution.clone());
    write_day_contributions(store, &contribution.day, &bucket)?;
    store.contributions.insert(
        contribution.id.clone(),
        ContributionMarker {
            analysis_version: contribution.analysis_version,
            day: contribution.day,
            audio_analyzed: contribution.audio.is_some(),
        },
    );
    Ok(())
}

fn remove_entry_unlocked(store: &mut InsightsStore, id: &str) -> Result<(), String> {
    if let Some(previous) = store.contributions.remove(id) {
        let mut bucket = read_day_contributions(store, &previous.day);
        bucket.remove(id);
        write_day_contributions(store, &previous.day, &bucket)?;
    }
    Ok(())
}

fn read_day_contributions(
    _store: &InsightsStore,
    day: &str,
) -> BTreeMap<String, InsightContribution> {
    #[cfg(test)]
    if BUCKET_DIR.get().is_none() {
        return _store
            .test_contributions
            .iter()
            .filter(|(_, contribution)| contribution.day == day)
            .map(|(id, contribution)| (id.clone(), contribution.clone()))
            .collect();
    }
    day_bucket_path(day)
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn read_day_aggregate(_store: &InsightsStore, day: &str) -> AggregateCore {
    #[cfg(test)]
    if BUCKET_DIR.get().is_none() {
        let mut aggregate = AggregateCore::default();
        for contribution in _store
            .test_contributions
            .values()
            .filter(|contribution| contribution.day == day)
        {
            aggregate.add_contribution(contribution);
        }
        return aggregate;
    }
    day_aggregate_path(day)
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn write_day_contributions(
    _store: &mut InsightsStore,
    day: &str,
    contributions: &BTreeMap<String, InsightContribution>,
) -> Result<(), String> {
    #[cfg(test)]
    if BUCKET_DIR.get().is_none() {
        _store
            .test_contributions
            .retain(|_, contribution| contribution.day != day);
        _store.test_contributions.extend(contributions.clone());
        update_day_index(_store, day, contributions.values());
        return Ok(());
    }
    let path =
        day_bucket_path(day).ok_or_else(|| "insights bucket path unavailable".to_string())?;
    let aggregate_path =
        day_aggregate_path(day).ok_or_else(|| "insights aggregate path unavailable".to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    if contributions.is_empty() {
        _store.daily.remove(day);
        if path.exists() {
            fs::remove_file(path).map_err(|error| error.to_string())?;
        }
        if aggregate_path.exists() {
            fs::remove_file(aggregate_path).map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    let mut aggregate = AggregateCore::default();
    for contribution in contributions.values() {
        aggregate.add_contribution(contribution);
    }
    write_json_atomic(&path, contributions)?;
    write_json_atomic(&aggregate_path, &aggregate)?;
    _store.daily.insert(
        day.to_string(),
        DailyBucketIndex {
            sessions: aggregate.sessions,
            words: aggregate.words,
            audio_sessions: aggregate.audio_samples.len() as u64,
        },
    );
    Ok(())
}

#[cfg(test)]
fn update_day_index<'a>(
    store: &mut InsightsStore,
    day: &str,
    contributions: impl Iterator<Item = &'a InsightContribution>,
) {
    let mut aggregate = AggregateCore::default();
    for contribution in contributions {
        aggregate.add_contribution(contribution);
    }
    if aggregate.sessions == 0 {
        store.daily.remove(day);
    } else {
        store.daily.insert(
            day.to_string(),
            DailyBucketIndex {
                sessions: aggregate.sessions,
                words: aggregate.words,
                audio_sessions: aggregate.audio_samples.len() as u64,
            },
        );
    }
}

fn day_bucket_path(day: &str) -> Option<PathBuf> {
    let safe = if day
        .chars()
        .all(|character| character.is_ascii_digit() || character == '-')
    {
        day
    } else {
        "unknown"
    };
    BUCKET_DIR
        .get()
        .map(|directory| directory.join(format!("{safe}.contributions.json")))
}

fn day_aggregate_path(day: &str) -> Option<PathBuf> {
    let safe = if day
        .chars()
        .all(|character| character.is_ascii_digit() || character == '-')
    {
        day
    } else {
        "unknown"
    };
    BUCKET_DIR
        .get()
        .map(|directory| directory.join(format!("{safe}.aggregate.json")))
}

fn write_json_atomic<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), String> {
    let temp = path.with_extension("json.tmp");
    fs::write(
        &temp,
        serde_json::to_vec(value).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    fs::rename(temp, path).map_err(|error| error.to_string())
}

fn clear_day_buckets() -> Result<(), String> {
    let Some(directory) = BUCKET_DIR.get() else {
        return Ok(());
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "json")
        {
            fs::remove_file(path).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn analyze_entry(entry: &HistoryEntry) -> InsightContribution {
    let text = entry
        .pipeline_runs
        .last()
        .map(|run| run.transcript.current())
        .filter(|text| !text.trim().is_empty())
        .unwrap_or(&entry.text);
    let tokens = tokenize(text);
    let language = detect_language(&tokens);
    let stopwords = stopwords(&language);
    let mut word_counts = BTreeMap::new();
    let mut content_word_counts = BTreeMap::new();
    for token in &tokens {
        *word_counts.entry(token.clone()).or_default() += 1;
        if !stopwords.contains(token.as_str()) && token.chars().count() > 1 {
            *content_word_counts.entry(token.clone()).or_default() += 1;
        }
    }
    let mut phrase_counts = BTreeMap::new();
    let mut phrase_sessions = BTreeSet::new();
    for width in 2..=4 {
        for window in tokens.windows(width) {
            let phrase = window.join(" ");
            *phrase_counts.entry(phrase.clone()).or_default() += 1;
            phrase_sessions.insert(phrase);
        }
    }
    let filler_counts = filler_counts(&tokens, &language);
    let raw_text = entry
        .pipeline_runs
        .last()
        .and_then(|run| run.transcript.raw.as_deref())
        .unwrap_or(text);
    let mut self_corrections = self_correction_count(raw_text, &language);
    if entry.pipeline_runs.last().is_some_and(|run| {
        run.journal
            .iter()
            .any(|stage| stage.stage == StageKind::Backtrack)
    }) {
        self_corrections = self_corrections.max(1);
    }
    let manual_corrections = entry
        .pipeline_runs
        .last()
        .and_then(|run| {
            run.transcript
                .user_corrected
                .as_ref()
                .zip(run.transcript.delivered.as_ref())
        })
        .map(|(after, before)| localized_change_count(before, after))
        .unwrap_or_default();
    let (day, hour, weekday) = parse_entry_time(&entry.date);
    let last_run = entry.pipeline_runs.last();
    let app = last_run
        .and_then(|run| run.context.process.as_deref())
        .map(normalize_application_name);
    let domain = last_run.and_then(|run| run.context.domain.clone());
    let category = classify_usage(entry, app.as_deref(), domain.as_deref());
    let audio = entry
        .audio_path
        .as_deref()
        .and_then(|path| analyze_audio_path(path).ok());
    let speech_ms = audio
        .as_ref()
        .map(|metrics| metrics.speech_duration_ms)
        .filter(|duration| *duration > 0)
        .unwrap_or(entry.duration_ms);
    let wpm = (speech_ms > 0 && !tokens.is_empty())
        .then_some(tokens.len() as f64 / (speech_ms as f64 / 60_000.0))
        .filter(|value| value.is_finite() && *value < 600.0);

    InsightContribution {
        id: entry.id.clone(),
        analysis_version: ANALYSIS_VERSION,
        day,
        hour,
        weekday,
        words: tokens.len() as u64,
        duration_ms: audio
            .as_ref()
            .map(|metrics| metrics.duration_ms)
            .unwrap_or(entry.duration_ms),
        manual_corrections,
        self_corrections,
        app,
        domain,
        category,
        language,
        word_counts,
        content_word_counts,
        phrase_counts,
        phrase_sessions,
        filler_counts,
        mattr: moving_average_type_token_ratio(&tokens, 50),
        wpm,
        audio,
    }
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if character.is_alphanumeric() || matches!(character, '\'' | '’' | '-') {
            current.extend(character.to_lowercase());
        } else if !current.is_empty() {
            let token = current.trim_matches(['\'', '’', '-']).to_string();
            if !token.is_empty() {
                tokens.push(token);
            }
            current.clear();
        }
    }
    if !current.is_empty() {
        let token = current.trim_matches(['\'', '’', '-']).to_string();
        if !token.is_empty() {
            tokens.push(token);
        }
    }
    tokens
}

fn detect_language(tokens: &[String]) -> String {
    let pt_markers = [
        "de", "que", "não", "para", "uma", "com", "por", "isso", "como", "mais",
    ];
    let en_markers = [
        "the", "and", "that", "for", "with", "this", "from", "not", "you", "are",
    ];
    let pt = tokens
        .iter()
        .filter(|token| pt_markers.contains(&token.as_str()))
        .count();
    let en = tokens
        .iter()
        .filter(|token| en_markers.contains(&token.as_str()))
        .count();
    if en > pt.saturating_mul(2) && en >= 2 {
        "en".into()
    } else {
        "pt-BR".into()
    }
}

fn stopwords(language: &str) -> BTreeSet<&'static str> {
    let words: &[&str] = if language == "en" {
        &[
            "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "has", "he", "in",
            "is", "it", "its", "of", "on", "that", "the", "to", "was", "were", "will", "with",
            "you", "your", "this", "not", "or", "we", "i",
        ]
    } else {
        &[
            "a", "à", "ao", "aos", "as", "às", "o", "os", "um", "uma", "uns", "umas", "de", "da",
            "das", "do", "dos", "e", "é", "em", "na", "nas", "no", "nos", "para", "por", "com",
            "sem", "que", "se", "eu", "você", "vocês", "ele", "ela", "eles", "elas", "me", "meu",
            "minha", "isso", "isto", "essa", "esse", "como", "mais", "mas", "ou", "já", "foi",
            "ser", "tem", "ter", "não", "sim",
        ]
    };
    words.iter().copied().collect()
}

fn filler_phrases(language: &str) -> &'static [&'static str] {
    if language == "en" {
        &[
            "like",
            "you know",
            "i mean",
            "basically",
            "actually",
            "kind of",
            "sort of",
        ]
    } else {
        &[
            "tipo",
            "né",
            "então",
            "assim",
            "no caso",
            "meio que",
            "digamos",
            "sabe",
            "basicamente",
            "na verdade",
        ]
    }
}

fn filler_counts(tokens: &[String], language: &str) -> BTreeMap<String, u64> {
    let mut counts = BTreeMap::new();
    for phrase in filler_phrases(language) {
        let needle: Vec<_> = phrase.split_whitespace().collect();
        let count = tokens
            .windows(needle.len())
            .filter(|window| window.iter().map(String::as_str).eq(needle.iter().copied()))
            .count() as u64;
        if count > 0 {
            counts.insert((*phrase).to_string(), count);
        }
    }
    counts
}

fn self_correction_count(text: &str, language: &str) -> u64 {
    let lower = text.to_lowercase();
    let markers: &[&str] = if language == "en" {
        &["no, i mean", "actually", "correction", "rather", "no, put"]
    } else {
        &[
            "não, quer dizer",
            "na verdade",
            "corrigindo",
            "melhor",
            "não, coloca",
            "não, para",
            "quer dizer",
        ]
    };
    markers
        .iter()
        .map(|marker| lower.matches(marker).count() as u64)
        .sum()
}

fn localized_change_count(before: &str, after: &str) -> u64 {
    let before_tokens = tokenize(before);
    let after_tokens = tokenize(after);
    if before_tokens == after_tokens {
        return 0;
    }
    // Equal-length edits can contain multiple separated replacements; count
    // their contiguous hunks. Insertions/deletions are conservatively treated
    // as one localized edit instead of inflating an alias such as
    // "open router" -> "OpenRouter" into two corrections.
    if before_tokens.len() != after_tokens.len() {
        return 1;
    }
    let mut changes = 0_u64;
    let mut inside_change = false;
    for (before, after) in before_tokens.iter().zip(&after_tokens) {
        if before != after {
            if !inside_change {
                changes += 1;
                inside_change = true;
            }
        } else {
            inside_change = false;
        }
    }
    changes
}

fn moving_average_type_token_ratio(tokens: &[String], window: usize) -> Option<f64> {
    if tokens.len() < 20 {
        return None;
    }
    let width = window.min(tokens.len());
    let mut sum = 0.0;
    let mut windows = 0_u64;
    for slice in tokens.windows(width) {
        let unique: BTreeSet<_> = slice.iter().collect();
        sum += unique.len() as f64 / width as f64;
        windows += 1;
    }
    (windows > 0).then_some(sum / windows as f64)
}

fn classify_usage(entry: &HistoryEntry, app: Option<&str>, domain: Option<&str>) -> String {
    let content = entry
        .pipeline_runs
        .last()
        .map(|run| format!("{:?}", run.content_hint).to_ascii_lowercase())
        .or_else(|| entry.content_type.clone())
        .unwrap_or_default();
    let app = app.unwrap_or_default().to_ascii_lowercase();
    let domain = domain.unwrap_or_default().to_ascii_lowercase();
    if content.contains("programming")
        || app.contains("code")
        || app.contains("codex")
        || domain.contains("github.com")
    {
        "Programming".into()
    } else if domain.contains("chatgpt") || domain.contains("gemini") || app.contains("codex") {
        "AI prompts".into()
    } else if domain.contains("gmail") || app.contains("outlook") {
        "Email".into()
    } else if content.contains("study") {
        "Study".into()
    } else if app.contains("whatsapp") || app.contains("telegram") {
        "Messages".into()
    } else if app.contains("word") || app.contains("notepad") || app.contains("obsidian") {
        "Documents".into()
    } else {
        "Unknown".into()
    }
}

fn normalize_application_name(value: &str) -> String {
    let file = Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(value);
    let stem = file.strip_suffix(".exe").unwrap_or(file);
    match stem.to_ascii_lowercase().as_str() {
        "chrome" => "Chrome".into(),
        "msedge" => "Microsoft Edge".into(),
        "code" => "VS Code".into(),
        "codex" => "Codex".into(),
        "whatsapp" => "WhatsApp".into(),
        "telegram" => "Telegram".into(),
        "outlook" | "olk" => "Outlook".into(),
        "winword" => "Microsoft Word".into(),
        "notepad" => "Notepad".into(),
        "obsidian" => "Obsidian".into(),
        _ => stem.to_string(),
    }
}

fn parse_entry_time(value: &str) -> (String, u8, u8) {
    let bytes = value.as_bytes();
    let day = if bytes.len() >= 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && value[..10].chars().all(|c| c.is_ascii_digit() || c == '-')
    {
        value[..10].to_string()
    } else {
        "unknown".into()
    };
    let hour = value
        .get(11..13)
        .and_then(|hour| hour.parse::<u8>().ok())
        .filter(|hour| *hour < 24)
        .unwrap_or(0);
    let weekday = parse_ymd(&day)
        .map(|(year, month, day)| weekday_from_date(year, month, day))
        .unwrap_or(0);
    (day, hour, weekday)
}

fn parse_ymd(value: &str) -> Option<(i32, u32, u32)> {
    let mut parts = value.split('-');
    Some((
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ))
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i32;
    let day = day as i32;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146097 + doe - 719468) as i64
}

fn weekday_from_date(year: i32, month: u32, day: u32) -> u8 {
    (days_from_civil(year, month, day) + 3).rem_euclid(7) as u8 // Monday = 0
}

fn analyze_audio_path(path: &str) -> Result<AudioMetrics, String> {
    let extension = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension != "wav" && extension != "wave" {
        return Err("unsupported historical audio format".into());
    }
    let bytes = crate::audio_store::read_original_or_canonical(path)?;
    let (samples, sample_rate) = decode_pcm16_wav(&bytes)?;
    Ok(analyze_pcm(&samples, sample_rate))
}

fn decode_pcm16_wav(bytes: &[u8]) -> Result<(Vec<i16>, u32), String> {
    if bytes.len() < 44 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("invalid WAV".into());
    }
    let mut offset = 12;
    let mut channels = 0_u16;
    let mut sample_rate = 0_u32;
    let mut bits = 0_u16;
    let mut format = 0_u16;
    let mut data = None;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let start = offset + 8;
        let end = start.saturating_add(size).min(bytes.len());
        if id == b"fmt " && end >= start + 16 {
            format = u16::from_le_bytes(bytes[start..start + 2].try_into().unwrap());
            channels = u16::from_le_bytes(bytes[start + 2..start + 4].try_into().unwrap());
            sample_rate = u32::from_le_bytes(bytes[start + 4..start + 8].try_into().unwrap());
            bits = u16::from_le_bytes(bytes[start + 14..start + 16].try_into().unwrap());
        } else if id == b"data" {
            data = Some(&bytes[start..end]);
        }
        offset = start + size + (size % 2);
    }
    if format != 1 || bits != 16 || channels == 0 || sample_rate == 0 {
        return Err("only PCM16 WAV is supported".into());
    }
    let data = data.ok_or_else(|| "WAV data chunk missing".to_string())?;
    if channels == 1 {
        return Ok((
            data.chunks_exact(2)
                .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                .collect(),
            sample_rate,
        ));
    }
    let frame_bytes = channels as usize * 2;
    let mono = data
        .chunks_exact(frame_bytes)
        .map(|frame| {
            let sum: i64 = frame
                .chunks_exact(2)
                .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as i64)
                .sum();
            (sum / channels as i64) as i16
        })
        .collect();
    Ok((mono, sample_rate))
}

fn analyze_pcm(samples: &[i16], sample_rate: u32) -> AudioMetrics {
    if samples.is_empty() || sample_rate == 0 {
        return AudioMetrics::default();
    }
    let normalized: Vec<f64> = samples
        .iter()
        .map(|sample| *sample as f64 / 32768.0)
        .collect();
    let mean_square =
        normalized.iter().map(|sample| sample * sample).sum::<f64>() / normalized.len() as f64;
    let peak = normalized
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0, f64::max);
    let clipping = normalized
        .iter()
        .filter(|sample| sample.abs() >= 0.999)
        .count() as f64
        / normalized.len() as f64;
    let frame_size = ((sample_rate as usize * 20) / 1000).max(1);
    let frame_db: Vec<f64> = normalized
        .chunks(frame_size)
        .map(|frame| {
            let energy =
                frame.iter().map(|sample| sample * sample).sum::<f64>() / frame.len().max(1) as f64;
            power_db(energy)
        })
        .collect();
    let noise_floor = percentile(&mut frame_db.clone(), 0.1).unwrap_or(-96.0);
    let speech_threshold = (noise_floor + 9.0).max(-45.0);
    let speech_flags: Vec<bool> = frame_db
        .iter()
        .map(|level| *level >= speech_threshold)
        .collect();
    let speech_frames = speech_flags.iter().filter(|active| **active).count();
    let silence_ratio = 1.0 - speech_frames as f64 / speech_flags.len().max(1) as f64;
    let speech_energy: Vec<f64> = frame_db
        .iter()
        .zip(&speech_flags)
        .filter_map(|(level, active)| active.then_some(*level))
        .collect();
    let speech_level = mean(&speech_energy).unwrap_or(power_db(mean_square));
    let snr = (speech_level - noise_floor).clamp(0.0, 80.0);
    let pauses = detect_pauses(&speech_flags, 20);
    let mut pause_values: Vec<f64> = pauses.iter().map(|value| *value as f64).collect();
    let f0 = estimate_pitch(&normalized, sample_rate, &speech_flags, frame_size);
    let mut f0_sorted = f0.clone();
    let f0_mean = mean(&f0);
    let f0_median = percentile(&mut f0_sorted, 0.5);
    let f0_stddev = f0_mean.map(|average| {
        (f0.iter()
            .map(|value| (value - average).powi(2))
            .sum::<f64>()
            / f0.len().max(1) as f64)
            .sqrt()
    });
    let pitch_range = if f0.len() >= 5 {
        let mut values = f0.clone();
        Some(
            percentile(&mut values, 0.9).unwrap_or_default()
                - percentile(&mut values, 0.1).unwrap_or_default(),
        )
    } else {
        None
    };
    AudioMetrics {
        duration_ms: normalized.len() as u64 * 1000 / sample_rate as u64,
        peak_dbfs: amplitude_db(peak),
        rms_dbfs: power_db(mean_square),
        lufs: -0.691 + power_db(mean_square),
        clipping_ratio: clipping,
        silence_ratio: silence_ratio.clamp(0.0, 1.0),
        speech_duration_ms: speech_frames as u64 * 20,
        noise_floor_estimate_dbfs: noise_floor,
        snr_estimate_db: snr,
        pause_count: pauses.len() as u32,
        mean_pause_duration_ms: mean(&pause_values),
        median_pause_duration_ms: percentile(&mut pause_values, 0.5),
        f0_mean_hz: f0_mean,
        f0_median_hz: f0_median,
        f0_stddev_hz: f0_stddev,
        pitch_range_hz: pitch_range,
    }
}

fn detect_pauses(flags: &[bool], frame_ms: u64) -> Vec<u64> {
    let mut pauses = Vec::new();
    let mut current = 0_u64;
    let mut seen_speech = false;
    for active in flags {
        if *active {
            if seen_speech && current >= 200 {
                pauses.push(current);
            }
            current = 0;
            seen_speech = true;
        } else if seen_speech {
            current += frame_ms;
        }
    }
    pauses
}

fn estimate_pitch(
    samples: &[f64],
    sample_rate: u32,
    speech_flags: &[bool],
    activity_frame_size: usize,
) -> Vec<f64> {
    let pitch_frame = ((sample_rate as usize * 30) / 1000).max(32);
    let step = ((sample_rate as usize * 40) / 1000).max(1);
    let min_lag = (sample_rate / 400).max(1) as usize;
    let max_lag = (sample_rate / 60).max(min_lag as u32 + 1) as usize;
    let mut pitches = Vec::new();
    for (frame_index, start) in (0..samples.len().saturating_sub(pitch_frame))
        .step_by(step)
        .take(3_000)
        .enumerate()
    {
        let activity_index = start / activity_frame_size;
        if !speech_flags.get(activity_index).copied().unwrap_or(false) {
            continue;
        }
        let frame = &samples[start..start + pitch_frame];
        let mean = frame.iter().sum::<f64>() / frame.len() as f64;
        let mut best = (0.0, 0_usize);
        for lag in (min_lag..max_lag.min(frame.len() / 2)).step_by(2) {
            let mut cross = 0.0;
            let mut left = 0.0;
            let mut right = 0.0;
            for index in (0..frame.len() - lag).step_by(2) {
                let a = frame[index] - mean;
                let b = frame[index + lag] - mean;
                cross += a * b;
                left += a * a;
                right += b * b;
            }
            let correlation = cross / (left * right).sqrt().max(1e-12);
            if correlation > best.0 {
                best = (correlation, lag);
            }
        }
        if best.0 >= 0.55 && best.1 > 0 {
            pitches.push(sample_rate as f64 / best.1 as f64);
        }
        let _ = frame_index;
    }
    pitches
}

fn amplitude_db(value: f64) -> f64 {
    20.0 * value.max(1e-9).log10()
}

fn power_db(value: f64) -> f64 {
    10.0 * value.max(1e-12).log10()
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn percentile(values: &mut [f64], percentile: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() - 1) as f64 * percentile.clamp(0.0, 1.0)).round() as usize;
    values.get(index).copied()
}

fn ranked(map: &BTreeMap<String, u64>, limit: usize) -> Vec<RankedCount> {
    let total: u64 = map.values().sum();
    let mut values: Vec<_> = map.iter().collect();
    values.sort_by(|left, right| right.1.cmp(left.1).then_with(|| left.0.cmp(right.0)));
    values
        .into_iter()
        .take(limit)
        .map(|(label, count)| RankedCount {
            label: label.clone(),
            count: *count,
            percentage: if total == 0 {
                0.0
            } else {
                *count as f64 * 100.0 / total as f64
            },
        })
        .collect()
}

fn aggregate_for_period(store: &InsightsStore, period: InsightPeriod) -> AggregateCore {
    if period == InsightPeriod::AllTime {
        return aggregate_between(store, i64::MIN, i64::MAX);
    }
    let today = current_local_day_number();
    let days = match period {
        InsightPeriod::Today => 1,
        InsightPeriod::Last7Days => 7,
        InsightPeriod::Last30Days => 30,
        InsightPeriod::AllTime => unreachable!(),
    };
    aggregate_between(store, today - days + 1, today)
}

fn previous_aggregate(store: &InsightsStore, period: InsightPeriod) -> AggregateCore {
    let today = current_local_day_number();
    let days = match period {
        InsightPeriod::Today => 1,
        InsightPeriod::Last7Days => 7,
        InsightPeriod::Last30Days | InsightPeriod::AllTime => 30,
    };
    aggregate_between(store, today - days * 2 + 1, today - days)
}

fn aggregate_between(store: &InsightsStore, start: i64, end: i64) -> AggregateCore {
    let mut aggregate = AggregateCore::default();
    for day_key in store.daily.keys() {
        let Some((year, month, day)) = parse_ymd(day_key) else {
            continue;
        };
        let number = days_from_civil(year, month, day);
        if number >= start && number <= end {
            aggregate.merge(&read_day_aggregate(store, day_key));
        }
    }
    aggregate
}

fn current_local_day_number() -> i64 {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::System::SystemInformation::GetLocalTime;
        let time = GetLocalTime();
        days_from_civil(time.wYear as i32, time.wMonth as u32, time.wDay as u32)
    }
    #[cfg(not(target_os = "windows"))]
    {
        (epoch_ms() / 86_400_000) as i64
    }
}

pub fn snapshot(
    period: InsightPeriod,
    vocabulary: &[crate::vocabulary::VocabularyTerm],
) -> InsightsResponse {
    let store = {
        let _guard = lock().lock();
        read_store_unlocked()
    };
    let aggregate = aggregate_for_period(&store, period);
    let previous = previous_aggregate(&store, period);
    let all_time = if period == InsightPeriod::AllTime {
        aggregate.clone()
    } else {
        aggregate_for_period(&store, InsightPeriod::AllTime)
    };
    build_response(&store, period, aggregate, previous, all_time, vocabulary)
}

fn build_response(
    store: &InsightsStore,
    period: InsightPeriod,
    aggregate: AggregateCore,
    previous: AggregateCore,
    all_time: AggregateCore,
    vocabulary: &[crate::vocabulary::VocabularyTerm],
) -> InsightsResponse {
    let mut wpm = aggregate.wpm_samples.clone();
    let average_wpm = mean(&wpm);
    let median_wpm = percentile(&mut wpm, 0.5);
    let typical_wpm = if wpm.len() >= 5 {
        let mut low = wpm.clone();
        let mut high = wpm.clone();
        Some([
            percentile(&mut low, 0.1).unwrap_or_default(),
            percentile(&mut high, 0.9).unwrap_or_default(),
        ])
    } else {
        None
    };
    let language = aggregate
        .language_counts
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(language, _)| language.clone())
        .unwrap_or_else(|| "pt-BR".into());
    let most_used_word = ranked(&aggregate.word_counts, 1).into_iter().next();
    let most_used_content_word = ranked(&aggregate.content_word_counts, 1).into_iter().next();
    let phrase_candidates: BTreeMap<_, _> = aggregate
        .phrase_counts
        .iter()
        .filter(|(phrase, _)| {
            aggregate.phrase_sessions.get(*phrase).copied().unwrap_or(0) >= 2
                && !phrase_is_empty(phrase, &language)
        })
        .map(|(phrase, count)| (phrase.clone(), *count))
        .collect();
    let most_used_phrase = ranked(&phrase_candidates, 1).into_iter().next();
    let catchphrase = catchphrase(&aggregate, &language);
    let fillers = ranked(&aggregate.filler_counts, 6)
        .into_iter()
        .map(|item| FillerInsight {
            phrase: item.label,
            count: item.count,
            per_1000_words: item.count as f64 * 1000.0 / aggregate.words.max(1) as f64,
        })
        .collect();
    let vocabulary_variety = (aggregate.mattr_weight > 0)
        .then_some(aggregate.mattr_weighted_sum / aggregate.mattr_weight as f64);
    let vocabulary_variety_label = vocabulary_variety.map(|value| {
        if value >= 0.72 {
            "Alta"
        } else if value >= 0.52 {
            "Moderada"
        } else {
            "Concentrada"
        }
        .to_string()
    });
    let correction_events = crate::learning::all();
    let period_bounds = period_bounds_ms(period);
    let event_count = |event: &crate::learning::CorrectionEvent| -> u64 {
        if period == InsightPeriod::AllTime {
            return event.count as u64;
        }
        event
            .occurrences_ms
            .iter()
            .filter(|timestamp| {
                period_bounds.is_some_and(|(start, end)| **timestamp >= start && **timestamp < end)
            })
            .count() as u64
    };
    let vocabulary_corrections = correction_events
        .iter()
        .filter(|event| event.status == crate::learning::SuggestionStatus::Accepted)
        .map(event_count)
        .sum();
    let most_corrected_event = correction_events
        .iter()
        .filter(|event| event_count(event) > 0)
        .max_by_key(|event| event_count(event));
    let most_corrected = most_corrected_event.map(|event| CorrectionInsight {
        before: event.before.clone(),
        after: event.after.clone(),
        count: event_count(event),
        in_vocabulary: vocabulary.iter().any(|term| {
            term.canonical.eq_ignore_ascii_case(&event.after)
                && term
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(&event.before))
        }),
    });
    let audio = summarize_audio(&aggregate);
    let temporal = summarize_temporal(store, &aggregate);
    let trends = summarize_trends(&aggregate, &previous);
    let applications = ranked(&aggregate.apps, 8);
    let application_details = applications
        .iter()
        .map(|application| {
            let prefix = format!("{}\u{1f}", application.label);
            let domains = aggregate
                .app_domains
                .iter()
                .filter_map(|(pair, count)| {
                    pair.strip_prefix(&prefix)
                        .map(|domain| (domain.to_string(), *count))
                })
                .collect::<BTreeMap<_, _>>();
            ApplicationInsight {
                name: application.label.clone(),
                count: application.count,
                percentage: application.percentage,
                domains: ranked(&domains, 5),
            }
        })
        .collect();
    InsightsResponse {
        analysis_version: ANALYSIS_VERSION,
        period,
        usage: UsageInsights {
            sessions: aggregate.sessions,
            words: aggregate.words,
            audio_duration_ms: aggregate.duration_ms,
            average_wpm,
            median_wpm,
            typical_wpm,
            manual_corrections: aggregate.manual_corrections,
            vocabulary_corrections,
        },
        language: LanguageInsights {
            language,
            most_used_word,
            most_used_content_word,
            most_used_phrase,
            catchphrase,
            fillers,
            self_corrections_per_1000_words: (aggregate.words >= 100)
                .then_some(aggregate.self_corrections as f64 * 1000.0 / aggregate.words as f64),
            vocabulary_variety,
            vocabulary_variety_label,
            most_corrected,
            catchphrase_ready: aggregate.sessions >= 5 && aggregate.words >= 500,
            profile_ready: all_time.words >= PROFILE_MIN_WORDS,
        },
        audio,
        applications,
        application_details,
        domains: ranked(&aggregate.domains, 8),
        categories: ranked(&aggregate.categories, 8),
        temporal,
        trends,
        profile_enabled: store.ai_profile_enabled,
        profile: store
            .ai_profile_enabled
            .then(|| store.profile.clone())
            .flatten(),
        profile_progress_words: all_time.words.min(PROFILE_MIN_WORDS),
        profile_required_words: PROFILE_MIN_WORDS,
        backfill: store.backfill.clone(),
        generated_at_ms: epoch_ms(),
    }
}

fn period_bounds_ms(period: InsightPeriod) -> Option<(u64, u64)> {
    if period == InsightPeriod::AllTime {
        return None;
    }
    let today = current_local_day_number();
    let days = match period {
        InsightPeriod::Today => 1,
        InsightPeriod::Last7Days => 7,
        InsightPeriod::Last30Days => 30,
        InsightPeriod::AllTime => unreachable!(),
    };
    let start_day = today - days + 1;
    Some((
        start_day.max(0) as u64 * 86_400_000,
        (today + 1).max(0) as u64 * 86_400_000,
    ))
}

fn phrase_is_empty(phrase: &str, language: &str) -> bool {
    let stop = stopwords(language);
    let tokens = tokenize(phrase);
    tokens.is_empty() || tokens.iter().all(|token| stop.contains(token.as_str()))
}

fn catchphrase(aggregate: &AggregateCore, language: &str) -> Option<RankedCount> {
    if aggregate.sessions < 5 || aggregate.words < 500 {
        return None;
    }
    let stop = stopwords(language);
    let filler: BTreeSet<_> = filler_phrases(language).iter().copied().collect();
    let mut best: Option<(&String, u64, f64)> = None;
    for (phrase, count) in &aggregate.phrase_counts {
        let sessions = aggregate.phrase_sessions.get(phrase).copied().unwrap_or(0);
        if sessions < 3 || phrase_is_empty(phrase, language) {
            continue;
        }
        let tokens = tokenize(phrase);
        let content_ratio = tokens
            .iter()
            .filter(|token| !stop.contains(token.as_str()))
            .count() as f64
            / tokens.len().max(1) as f64;
        let filler_penalty = if filler.contains(phrase.as_str()) {
            0.55
        } else {
            1.0
        };
        let score =
            sessions as f64 * (*count as f64 + 1.0).ln() * (0.55 + content_ratio) * filler_penalty;
        if best.is_none() || score > best.as_ref().unwrap().2 {
            best = Some((phrase, *count, score));
        }
    }
    best.map(|(phrase, count, _)| RankedCount {
        label: phrase.clone(),
        count,
        percentage: count as f64 * 100.0 / aggregate.sessions.max(1) as f64,
    })
}

fn summarize_audio(aggregate: &AggregateCore) -> AudioInsights {
    let samples = &aggregate.audio_samples;
    let med = |selector: fn(&AudioMetrics) -> f64| {
        let mut values: Vec<_> = samples.iter().map(selector).collect();
        percentile(&mut values, 0.5)
    };
    let lufs_typical = if samples.len() >= 5 {
        let mut values: Vec<_> = samples.iter().map(|sample| sample.lufs).collect();
        let mut high = values.clone();
        Some([
            percentile(&mut values, 0.1).unwrap_or_default(),
            percentile(&mut high, 0.9).unwrap_or_default(),
        ])
    } else {
        None
    };
    let clipping = mean(
        &samples
            .iter()
            .map(|sample| sample.clipping_ratio)
            .collect::<Vec<_>>(),
    );
    let silence = mean(
        &samples
            .iter()
            .map(|sample| sample.silence_ratio)
            .collect::<Vec<_>>(),
    );
    let snr = med(|sample| sample.snr_estimate_db);
    let pause = mean(
        &samples
            .iter()
            .filter_map(|sample| sample.mean_pause_duration_ms)
            .collect::<Vec<_>>(),
    );
    let pitch_median = {
        let mut values: Vec<_> = samples
            .iter()
            .filter_map(|sample| sample.f0_median_hz)
            .collect();
        percentile(&mut values, 0.5)
    };
    let pitch_variation = {
        let ratios: Vec<_> = samples
            .iter()
            .filter_map(|sample| {
                sample
                    .f0_stddev_hz
                    .zip(sample.f0_median_hz)
                    .map(|(stddev, median)| stddev / median.max(1.0))
            })
            .collect();
        mean(&ratios).map(|ratio| {
            if ratio < 0.08 {
                "Baixa"
            } else if ratio < 0.18 {
                "Moderada"
            } else {
                "Alta"
            }
            .to_string()
        })
    };
    let lufs = med(|sample| sample.lufs);
    let quality = if samples.len() < 2 {
        None
    } else if clipping.unwrap_or_default() > 0.01 || snr.unwrap_or(30.0) < 10.0 {
        Some("Atenção".into())
    } else if snr.unwrap_or_default() >= 20.0
        && clipping.unwrap_or_default() < 0.002
        && lufs.unwrap_or(-30.0) > -36.0
    {
        Some("Boa".into())
    } else {
        Some("Estável".into())
    };
    AudioInsights {
        analyzed_sessions: samples.len() as u64,
        coverage_percentage: if aggregate.sessions == 0 {
            0.0
        } else {
            samples.len() as f64 * 100.0 / aggregate.sessions as f64
        },
        lufs_median: lufs,
        lufs_typical,
        rms_dbfs_median: med(|sample| sample.rms_dbfs),
        peak_dbfs_median: med(|sample| sample.peak_dbfs),
        clipping_ratio: clipping,
        silence_ratio: silence,
        speech_ratio: silence.map(|value| 1.0 - value),
        estimated_snr_db: snr,
        average_pause_ms: pause,
        median_f0_hz: pitch_median,
        pitch_variation,
        capture_quality: quality,
    }
}

fn summarize_temporal(store: &InsightsStore, aggregate: &AggregateCore) -> TemporalInsights {
    let peak_hour = aggregate
        .hours
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(hour, _)| *hour);
    let peak_weekday = aggregate
        .weekdays
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(day, _)| *day);
    let mut days: Vec<i64> = store
        .daily
        .iter()
        .filter(|(_, bucket)| bucket.sessions > 0)
        .filter_map(|(day, _)| parse_ymd(day))
        .map(|(year, month, day)| days_from_civil(year, month, day))
        .collect();
    days.sort_unstable();
    days.dedup();
    let mut longest = 0_u64;
    let mut running = 0_u64;
    let mut previous = None;
    for day in &days {
        running = if previous.is_some_and(|previous| *day == previous + 1) {
            running + 1
        } else {
            1
        };
        longest = longest.max(running);
        previous = Some(*day);
    }
    let today = current_local_day_number();
    let mut cursor = if days.binary_search(&today).is_ok() {
        today
    } else {
        today - 1
    };
    let day_set: BTreeSet<_> = days.into_iter().collect();
    let mut current = 0_u64;
    while day_set.contains(&cursor) {
        current += 1;
        cursor -= 1;
    }
    let activity = store
        .daily
        .iter()
        .rev()
        .take(91)
        .map(|(day, bucket)| DailyActivity {
            day: day.clone(),
            sessions: bucket.sessions,
            words: bucket.words,
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    TemporalInsights {
        peak_hour,
        peak_weekday,
        current_streak_days: current,
        longest_streak_days: longest,
        activity,
    }
}

fn summarize_trends(current: &AggregateCore, previous: &AggregateCore) -> Vec<MetricTrend> {
    if current.sessions < MIN_TREND_SESSIONS || previous.sessions < MIN_TREND_SESSIONS {
        return Vec::new();
    }
    let mut trends = Vec::new();
    push_trend(
        &mut trends,
        "speaking_speed_wpm",
        mean(&current.wpm_samples),
        mean(&previous.wpm_samples),
    );
    push_trend(
        &mut trends,
        "voice_level_lufs",
        mean(
            &current
                .audio_samples
                .iter()
                .map(|sample| sample.lufs)
                .collect::<Vec<_>>(),
        ),
        mean(
            &previous
                .audio_samples
                .iter()
                .map(|sample| sample.lufs)
                .collect::<Vec<_>>(),
        ),
    );
    push_trend(
        &mut trends,
        "corrections_per_1000_words",
        (current.words > 0)
            .then_some(current.manual_corrections as f64 * 1000.0 / current.words as f64),
        (previous.words > 0)
            .then_some(previous.manual_corrections as f64 * 1000.0 / previous.words as f64),
    );
    let current_fillers: u64 = current.filler_counts.values().sum();
    let previous_fillers: u64 = previous.filler_counts.values().sum();
    push_trend(
        &mut trends,
        "fillers_per_1000_words",
        (current.words > 0).then_some(current_fillers as f64 * 1000.0 / current.words as f64),
        (previous.words > 0).then_some(previous_fillers as f64 * 1000.0 / previous.words as f64),
    );
    trends
}

fn push_trend(
    target: &mut Vec<MetricTrend>,
    metric: &str,
    current: Option<f64>,
    previous: Option<f64>,
) {
    let Some((current, previous)) = current.zip(previous) else {
        return;
    };
    let change_absolute = current - previous;
    let change_percent =
        (previous.abs() > 1e-6).then_some(change_absolute / previous.abs() * 100.0);
    target.push(MetricTrend {
        metric: metric.into(),
        current,
        previous,
        change_percent,
        change_absolute,
    });
}

pub fn set_profile_enabled(enabled: bool) -> Result<(), String> {
    let _guard = lock().lock();
    let mut store = read_store_unlocked();
    store.ai_profile_enabled = enabled;
    write_store_unlocked(&store)
}

pub async fn generate_voice_profile(state: &AppState) -> Result<VoiceProfile, String> {
    let store = {
        let _guard = lock().lock();
        read_store_unlocked()
    };
    let enabled = store.ai_profile_enabled;
    let previous_profile = store.profile.clone();
    let aggregate = aggregate_for_period(&store, InsightPeriod::AllTime);
    if !enabled {
        return Err("Ative o Voice Profile por IA antes de gerar.".into());
    }
    if aggregate.words < PROFILE_MIN_WORDS {
        return Err(format!(
            "São necessárias pelo menos {PROFILE_MIN_WORDS} palavras para gerar um perfil confiável."
        ));
    }
    if previous_profile
        .as_ref()
        .is_some_and(|profile| epoch_ms().saturating_sub(profile.generated_at_ms) < 60_000)
    {
        return Err("Aguarde um minuto antes de regenerar o perfil.".into());
    }
    let api_key = state
        .next_openrouter_key()
        .ok_or_else(|| "Configure uma chave do OpenRouter em Provedores e APIs.".to_string())?;
    let metrics = voice_profile_payload(&aggregate);
    let system = "Você gera um perfil descritivo de uso de ditado. Responda SOMENTE JSON válido com title e description em pt-BR. Use apenas os dados agregados fornecidos. Não infira personalidade, saúde, emoção, identidade ou atributos sensíveis. O título deve ter 2 a 4 palavras e a descrição no máximo 420 caracteres, com linguagem não julgadora.";
    let user = format!(
        "Métricas locais agregadas do Voice Insights (sem histórico bruto):\n{}",
        serde_json::to_string(&metrics).map_err(|error| error.to_string())?
    );
    let mut attempts = Vec::new();
    let mut selected_model = VOICE_PROFILE_MODEL;
    let mut selected_result = None;
    for model in [VOICE_PROFILE_MODEL, VOICE_PROFILE_FALLBACK_MODEL] {
        let started = std::time::Instant::now();
        match crate::openrouter::generate_text(
            system,
            &user,
            model,
            &api_key,
            Duration::from_secs(45),
        )
        .await
        {
            Ok(result) => {
                attempts.push(VoiceProfileAttempt {
                    provider: "OpenRouter".into(),
                    model: model.into(),
                    status: "success".into(),
                    duration_ms: started.elapsed().as_millis() as u64,
                    error: None,
                });
                selected_model = model;
                selected_result = Some(result);
                break;
            }
            Err(error) => attempts.push(VoiceProfileAttempt {
                provider: "OpenRouter".into(),
                model: model.into(),
                status: "failed".into(),
                duration_ms: started.elapsed().as_millis() as u64,
                error: Some(error),
            }),
        }
    }
    let result = selected_result.ok_or_else(|| {
        "Voice Profile: o modelo principal e o fallback falharam; consulte os detalhes técnicos."
            .to_string()
    })?;
    #[derive(Deserialize)]
    struct ProfileResponse {
        title: String,
        description: String,
    }
    let cleaned = result
        .text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let parsed: ProfileResponse = serde_json::from_str(cleaned)
        .map_err(|_| "O provider retornou um perfil inválido.".to_string())?;
    if parsed.title.trim().is_empty()
        || parsed.title.chars().count() > 60
        || parsed.description.trim().is_empty()
        || parsed.description.chars().count() > 500
    {
        return Err("O provider retornou um perfil fora dos limites esperados.".into());
    }
    let profile = VoiceProfile {
        title: parsed.title.trim().to_string(),
        description: parsed.description.trim().to_string(),
        generated_at_ms: epoch_ms(),
        generated_at_word_count: aggregate.words,
        next_update_word_count: aggregate.words + PROFILE_REFRESH_WORDS,
        profile_version: 1,
        provider: "OpenRouter".into(),
        model: selected_model.into(),
        request_ms: result.request_ms,
        ttfb_ms: result.ttfb_ms,
        reported_total_tokens: result.reported_total_tokens,
        reported_input_tokens: result.reported_input_tokens,
        reported_output_tokens: result.reported_output_tokens,
        reported_cost_usd: result.reported_cost_usd,
        generation_id: result.generation_id,
        bytes_sent: result.bytes_sent,
        attempts,
    };
    let _guard = lock().lock();
    let mut store = read_store_unlocked();
    store.profile = Some(profile.clone());
    write_store_unlocked(&store)?;
    Ok(profile)
}

fn voice_profile_payload(aggregate: &AggregateCore) -> serde_json::Value {
    serde_json::json!({
        "sessions": aggregate.sessions,
        "words": aggregate.words,
        "average_wpm": mean(&aggregate.wpm_samples),
        "top_content_words": safe_ranked(&aggregate.content_word_counts, 12),
        "top_phrases": safe_ranked(&aggregate.phrase_counts, 8),
        "applications": ranked(&aggregate.apps, 8),
        "categories": ranked(&aggregate.categories, 8),
        "manual_corrections": aggregate.manual_corrections,
        "self_corrections_per_1000_words": if aggregate.words > 0 { Some(aggregate.self_corrections as f64 * 1000.0 / aggregate.words as f64) } else { None },
        "vocabulary_variety_mattr": if aggregate.mattr_weight > 0 { Some(aggregate.mattr_weighted_sum / aggregate.mattr_weight as f64) } else { None },
        "audio": summarize_audio(aggregate),
    })
}

fn safe_ranked(map: &BTreeMap<String, u64>, limit: usize) -> Vec<RankedCount> {
    let safe: BTreeMap<_, _> = map
        .iter()
        .filter(|(label, _)| !looks_sensitive_term(label))
        .map(|(label, count)| (label.clone(), *count))
        .collect();
    ranked(&safe, limit)
}

fn looks_sensitive_term(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if [
        "api_key", "apikey", "password", "passwd", "secret", "bearer", "token",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return true;
    }
    value.split_whitespace().any(|token| {
        if token.chars().count() < 20 {
            return false;
        }
        let length = token.chars().count() as f64;
        let has_lower = token.chars().any(char::is_lowercase);
        let has_upper = token.chars().any(char::is_uppercase);
        let has_digit = token.chars().any(|character| character.is_ascii_digit());
        let has_symbol = token
            .chars()
            .any(|character| !character.is_alphanumeric() && !matches!(character, '-' | '_' | '.'));
        let class_count = [has_lower, has_upper, has_digit, has_symbol]
            .into_iter()
            .filter(|present| *present)
            .count();
        let mut frequencies = BTreeMap::new();
        for character in token.chars() {
            *frequencies.entry(character).or_insert(0_u64) += 1;
        }
        let entropy = frequencies.values().fold(0.0, |total, count| {
            let probability = *count as f64 / length;
            total - probability * probability.log2()
        });
        let unique_ratio = frequencies.len() as f64 / length;
        has_symbol
            || (class_count >= 3 && entropy >= 3.0)
            || (class_count >= 2 && entropy >= 3.6 && unique_ratio >= 0.45)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, date: &str, text: &str) -> HistoryEntry {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "date": date,
            "words": text.split_whitespace().count(),
            "engine": "test",
            "text": text,
            "duration_ms": 60000,
            "source": "mic",
            "latency_ms": 10,
            "pipeline_runs": []
        }))
        .unwrap()
    }

    #[test]
    fn stopwords_do_not_win_content_frequency() {
        let contribution = analyze_entry(&entry(
            "1",
            "2026-08-26 10:00",
            "de de de pipeline pipeline",
        ));
        assert_eq!(contribution.word_counts["de"], 3);
        assert_eq!(contribution.content_word_counts["pipeline"], 2);
        assert!(!contribution.content_word_counts.contains_key("de"));
    }

    #[test]
    fn phrases_track_distribution_across_sessions() {
        let first = analyze_entry(&entry(
            "1",
            "2026-08-26 10:00",
            "no caso vamos testar no caso",
        ));
        let second = analyze_entry(&entry("2", "2026-08-26 11:00", "no caso funciona"));
        let mut aggregate = AggregateCore::default();
        aggregate.add_contribution(&first);
        aggregate.add_contribution(&second);
        assert_eq!(aggregate.phrase_counts["no caso"], 3);
        assert_eq!(aggregate.phrase_sessions["no caso"], 2);
    }

    #[test]
    fn filler_rate_and_self_corrections_are_descriptive() {
        let contribution = analyze_entry(&entry(
            "1",
            "2026-08-26 10:00",
            "tipo eu pensei não, quer dizer, no caso eu revisei",
        ));
        assert_eq!(contribution.filler_counts["tipo"], 1);
        assert_eq!(contribution.filler_counts["no caso"], 1);
        assert!(contribution.self_corrections >= 1);
    }

    #[test]
    fn mattr_uses_windows_instead_of_whole_corpus_ttr() {
        let tokens: Vec<_> = (0..100)
            .map(|index| format!("word{}", index % 30))
            .collect();
        let value = moving_average_type_token_ratio(&tokens, 50).unwrap();
        assert!(value > 0.4 && value < 0.8);
    }

    #[test]
    fn edit_and_delete_recompute_only_affected_bucket() {
        let mut store = InsightsStore::default();
        upsert_entry_unlocked(&mut store, &entry("1", "2026-08-26 10:00", "um dois tres"));
        assert_eq!(
            aggregate_for_period(&store, InsightPeriod::AllTime).words,
            3
        );
        upsert_entry_unlocked(&mut store, &entry("1", "2026-08-26 10:00", "um dois"));
        assert_eq!(
            aggregate_for_period(&store, InsightPeriod::AllTime).words,
            2
        );
        remove_entry_unlocked(&mut store, "1").unwrap();
        assert_eq!(
            aggregate_for_period(&store, InsightPeriod::AllTime).words,
            0
        );
        assert!(store.daily.is_empty());
    }

    #[test]
    fn user_corrected_version_updates_correction_aggregate_without_overwriting_history() {
        let mut corrected = entry("1", "2026-08-26 10:00", "open router funciona");
        let mut run = crate::pipeline_run::PipelineRun::success(
            "run-1",
            crate::pipeline_contract::TranscriptionMode::UltraFast,
            "open router funciona",
        );
        run.transcript.user_corrected = Some("OpenRouter funciona".into());
        corrected.pipeline_runs.push(run);

        let contribution = analyze_entry(&corrected);
        assert_eq!(contribution.manual_corrections, 1);
        assert_eq!(contribution.word_counts["openrouter"], 1);
        assert_eq!(
            corrected.pipeline_runs[0].transcript.raw.as_deref(),
            Some("open router funciona")
        );
    }

    #[test]
    fn main_projection_stays_compact_and_daily_details_are_separate() {
        let mut store = InsightsStore::default();
        upsert_entry_unlocked(
            &mut store,
            &entry(
                "1",
                "2026-08-26 10:00",
                "pipeline arquitetura incremental observabilidade",
            ),
        );
        let persisted = serde_json::to_string(&store).unwrap();
        assert!(!persisted.contains("observabilidade"));
        assert!(persisted.len() < 2_000);
        assert_eq!(
            aggregate_for_period(&store, InsightPeriod::AllTime).words,
            4
        );
    }

    #[test]
    fn moving_an_entry_rebuilds_both_daily_buckets() {
        let mut store = InsightsStore::default();
        upsert_entry_unlocked(&mut store, &entry("1", "2026-08-25 10:00", "um dois"));
        upsert_entry_unlocked(&mut store, &entry("1", "2026-08-26 10:00", "um dois tres"));
        assert!(!store.daily.contains_key("2026-08-25"));
        assert_eq!(store.daily["2026-08-26"].words, 3);
    }

    #[test]
    fn wav_metrics_detect_level_silence_clipping_and_pitch() {
        let sample_rate = 16_000;
        let mut samples = vec![0_i16; sample_rate as usize / 2];
        samples.extend((0..sample_rate).map(|index| {
            let phase = index as f64 * std::f64::consts::TAU * 200.0 / sample_rate as f64;
            (phase.sin() * 0.5 * i16::MAX as f64) as i16
        }));
        samples.extend(vec![i16::MAX; 160]);
        let metrics = analyze_pcm(&samples, sample_rate);
        assert!(metrics.peak_dbfs > -0.1);
        assert!(metrics.rms_dbfs < -3.0);
        assert!(metrics.clipping_ratio > 0.0);
        assert!(metrics.silence_ratio > 0.1);
        assert!(metrics
            .f0_median_hz
            .is_some_and(|value| (value - 200.0).abs() < 20.0));
    }

    #[test]
    fn analysis_version_marks_contribution() {
        let contribution = analyze_entry(&entry("1", "2026-08-26 10:00", "teste de versão"));
        assert_eq!(contribution.analysis_version, ANALYSIS_VERSION);
    }

    #[test]
    fn wpm_uses_available_speech_duration() {
        let text = (0..120).map(|_| "palavra").collect::<Vec<_>>().join(" ");
        let contribution = analyze_entry(&entry("1", "2026-08-26 10:00", &text));
        assert!(contribution
            .wpm
            .is_some_and(|value| (value - 120.0).abs() < 0.01));
    }

    #[test]
    fn catchphrase_requires_distribution_and_enough_sample() {
        let mut aggregate = AggregateCore::default();
        for index in 0..5 {
            let text = format!(
                "{} arquitetura incremental com cuidado arquitetura incremental {}",
                "contexto ".repeat(105),
                index
            );
            aggregate.add_contribution(&analyze_entry(&entry(
                &index.to_string(),
                "2026-08-26 10:00",
                &text,
            )));
        }
        let result = catchphrase(&aggregate, "pt-BR").expect("catchphrase should be ready");
        assert!(result.label.contains("arquitetura") || result.label.contains("contexto"));
    }

    #[test]
    fn interrupted_backfill_resumes_only_missing_or_stale_entries() {
        let entries = vec![
            entry("done", "2026-08-25 10:00", "já analisado"),
            entry("missing", "2026-08-26 10:00", "ainda pendente"),
            entry("stale", "2026-08-26 11:00", "versão antiga"),
        ];
        let mut store = InsightsStore::default();
        upsert_entry_unlocked(&mut store, &entries[0]);
        store.contributions.insert(
            entries[2].id.clone(),
            ContributionMarker {
                analysis_version: ANALYSIS_VERSION.saturating_sub(1),
                day: "2026-08-26".into(),
                audio_analyzed: false,
            },
        );
        let current = current_contribution_ids(&store, &entries);
        assert_eq!(current, BTreeSet::from(["done".to_string()]));
    }

    #[test]
    fn profile_payload_excludes_secret_like_terms_and_raw_transcripts() {
        let mut aggregate = AggregateCore::default();
        aggregate.content_word_counts.insert("pipeline".into(), 4);
        aggregate
            .content_word_counts
            .insert("api_key_skABCDef123456789012345".into(), 12);
        aggregate
            .content_word_counts
            .insert("a9f2c7e1b8d4f6a3c5e7b9d2".into(), 9);
        let payload = voice_profile_payload(&aggregate).to_string();
        assert!(payload.contains("pipeline"));
        assert!(!payload.contains("skABC"));
        assert!(!payload.contains("a9f2c7e1"));
        assert!(!payload.contains("transcript"));
    }

    #[test]
    fn application_names_are_local_and_human_readable() {
        assert_eq!(
            normalize_application_name(r"C:\Program Files\Chrome\chrome.exe"),
            "Chrome"
        );
        assert_eq!(normalize_application_name("Code.exe"), "VS Code");
        assert_eq!(
            normalize_application_name("custom-editor.exe"),
            "custom-editor"
        );
    }
}
