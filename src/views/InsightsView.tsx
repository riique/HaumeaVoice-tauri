import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Activity,
  AudioLines,
  BrainCircuit,
  Check,
  ChevronDown,
  Clock3,
  Info,
  Loader2,
  Pause,
  Play,
  RefreshCw,
  Sparkles,
  Volume2,
} from "lucide-react";
import { Button } from "../components/ui/Button";
import { Toggle } from "../components/ui/Toggle";
import { ErrorState, PageHeader, SkeletonRows } from "../components/ui/Surface";
import {
  adjacentInsightsTab,
  buildActivityCells,
  formatInsightNumber as number,
  voiceEvidenceLevelLabel,
  voiceProfileProgress,
  voiceProfileRemainingWords,
  type InsightsTab,
} from "./insights-utils";
import {
  addInsightCorrectionToVocabulary,
  generateAiVoiceProfile,
  getDevMode,
  getInsights,
  setAiVoiceProfileEnabled,
  setInsightsBackfillPaused,
  type ApplicationInsight,
  type InsightPeriod,
  type InsightsResponse,
  type MetricTrend,
  type RankedCount,
} from "../lib/tauri";

type Tab = InsightsTab;

const PERIODS: Array<{ value: InsightPeriod; label: string }> = [
  { value: "today", label: "Hoje" },
  { value: "last7_days", label: "7 dias" },
  { value: "last30_days", label: "30 dias" },
  { value: "all_time", label: "Todo o período" },
];

const WEEKDAYS = ["segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado", "domingo"];

function duration(milliseconds: number) {
  const minutes = Math.round(milliseconds / 60_000);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  const remaining = minutes % 60;
  return remaining ? `${hours} h ${remaining} min` : `${hours} h`;
}

function signed(value: number, suffix = "%") {
  return `${value > 0 ? "+" : ""}${number(value, 1)}${suffix}`;
}

function MetricHelp({ label }: { label: string }) {
  return (
    <span className="inline-flex cursor-help align-middle text-[#85867f] outline-none focus-visible:ring-2 focus-visible:ring-[#777870]" title={label} aria-label={label} role="note" tabIndex={0}>
      <Info className="h-3.5 w-3.5" aria-hidden />
    </span>
  );
}

function TrendBadge({ trend, absolute = false }: { trend?: MetricTrend; absolute?: boolean }) {
  if (!trend) return <span className="text-[11px] text-muted">Amostra insuficiente para tendência</span>;
  const value = absolute || trend.change_percent == null ? signed(trend.change_absolute, "") : signed(trend.change_percent);
  return <span className="text-[11px] tabular-nums text-[#555650]">{value} vs. período anterior</span>;
}

function RankedRows({ items, empty = "Ainda não há dados suficientes." }: { items: RankedCount[]; empty?: string }) {
  if (!items.length) return <p className="py-8 text-[13px] text-muted">{empty}</p>;
  return (
    <div className="divider-list">
      {items.map((item) => (
        <div key={item.label} className="grid min-h-12 grid-cols-[minmax(0,1fr)_120px_48px] items-center gap-4 py-2.5">
          <span className="truncate text-[13px] text-[#343530]" title={item.label}>{item.label}</span>
          <span className="h-1.5 overflow-hidden rounded-full bg-[#e7e7e1]" aria-hidden>
            <span className="block h-full rounded-full bg-[#777870]" style={{ width: `${Math.max(3, item.percentage)}%` }} />
          </span>
          <span className="text-right font-mono text-[11px] tabular-nums text-muted">{number(item.percentage, 0)}%</span>
        </div>
      ))}
    </div>
  );
}

function ApplicationRows({ items }: { items: ApplicationInsight[] }) {
  const [expanded, setExpanded] = useState<string | null>(null);
  const listId = useRef(`insight-apps-${crypto.randomUUID()}`);
  if (!items.length) return <p className="py-8 text-[13px] text-muted">O aplicativo de destino não estava disponível nas gravações deste período.</p>;
  return (
    <div className="divider-list">
      {items.map((item, index) => {
        const open = expanded === item.name;
        const domainsId = `${listId.current}-${index}`;
        return (
          <div key={item.name} className="py-3">
            <button
              type="button"
              className="grid min-h-10 w-full grid-cols-[minmax(0,1fr)_150px_52px_18px] items-center gap-4 text-left"
              onClick={() => item.domains.length && setExpanded(open ? null : item.name)}
              disabled={!item.domains.length}
              aria-expanded={item.domains.length ? open : undefined}
              aria-controls={item.domains.length ? domainsId : undefined}
            >
              <span className="truncate text-[13px] font-medium text-[#343530]" title={item.name}>{item.name}</span>
              <span className="h-1.5 overflow-hidden rounded-full bg-[#e7e7e1]" aria-hidden>
                <span className="block h-full rounded-full bg-[#555650]" style={{ width: `${Math.max(3, item.percentage)}%` }} />
              </span>
              <span className="text-right font-mono text-[11px] tabular-nums text-muted">{number(item.percentage)}%</span>
              {item.domains.length ? <ChevronDown className={`h-4 w-4 text-muted transition-transform ${open ? "rotate-180" : ""}`} aria-hidden /> : <span />}
            </button>
            {open && (
              <div id={domainsId} className="ml-4 mt-3 border-l border-line pl-4">
                {item.domains.map((domain) => (
                  <div key={domain.label} className="flex items-center justify-between py-1.5 text-[13px] text-muted">
                    <span className="truncate" title={domain.label}>{domain.label}</span>
                    <span className="font-mono text-[11px] tabular-nums">{domain.count} ditados</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

function ActivityCalendar({ activity }: { activity: InsightsResponse["temporal"]["activity"] }) {
  const cells = useMemo(() => buildActivityCells(activity), [activity]);
  const max = Math.max(1, ...cells.map((cell) => cell.count));
  const activeDays = cells.filter((cell) => cell.count > 0).length;
  const totalSessions = cells.reduce((total, cell) => total + cell.count, 0);
  return (
    <div className="mt-5 grid grid-flow-col grid-rows-7 gap-1" aria-label={`Atividade nos últimos 91 dias: ${totalSessions} ditados em ${activeDays} dias ativos`}>
      {cells.map((cell) => {
        const strength = cell.count / max;
        const background = cell.count === 0 ? "#ecece7" : strength > .66 ? "#4c4d48" : strength > .33 ? "#85867f" : "#b9bab3";
        return <span key={cell.key} className="aspect-square min-w-0 rounded-[3px]" style={{ background }} title={`${cell.key}: ${cell.count} ditados`} aria-hidden />;
      })}
    </div>
  );
}

function UsageTab({ data }: { data: InsightsResponse }) {
  const wpmTrend = data.trends.find((trend) => trend.metric === "speaking_speed_wpm");
  return (
    <div className="space-y-10">
      <section className="surface overflow-hidden">
        <div className="grid grid-cols-[1.35fr_1fr_1fr] max-[940px]:grid-cols-1">
          <div className="border-r border-line p-7 max-[940px]:border-b max-[940px]:border-r-0">
            <div className="flex items-center gap-1.5 text-[13px] text-muted">Velocidade média <MetricHelp label="Palavras divididas pelo tempo estimado de fala, excluindo silêncio detectado quando há áudio analisado." /></div>
            <div className="mt-4 flex items-baseline gap-2"><strong className="text-[42px] font-semibold tracking-[-0.045em] tabular-nums text-ink">{data.usage.average_wpm ? number(data.usage.average_wpm) : "—"}</strong><span className="text-[13px] text-muted">PPM</span></div>
            <div className="mt-2"><TrendBadge trend={wpmTrend} /></div>
            {data.usage.typical_wpm && <p className="mt-5 text-[13px] text-muted">Faixa típica {number(data.usage.typical_wpm[0])}–{number(data.usage.typical_wpm[1])} PPM</p>}
          </div>
          <div className="border-r border-line p-7 max-[940px]:border-b max-[940px]:border-r-0">
            <div className="text-[13px] text-muted">Correções manuais</div>
            <div className="mt-4 text-[32px] font-semibold tracking-[-0.03em] tabular-nums">{number(data.usage.manual_corrections)}</div>
            <p className="mt-2 text-[13px] leading-5 text-muted">{number(data.usage.vocabulary_corrections)} correções já incorporadas ao vocabulário.</p>
          </div>
          <div className="p-7">
            <div className="text-[13px] text-muted">Palavras ditadas</div>
            <div className="mt-4 text-[32px] font-semibold tracking-[-0.03em] tabular-nums">{number(data.usage.words)}</div>
            <p className="mt-2 text-[13px] leading-5 text-muted">{number(data.usage.sessions)} ditados · {duration(data.usage.audio_duration_ms)} de áudio</p>
          </div>
        </div>
      </section>

      <section className="grid grid-cols-[minmax(0,1.45fr)_minmax(280px,.75fr)] gap-8 max-[900px]:grid-cols-1">
        <div>
          <h2 className="section-title">Onde você dita</h2>
          <p className="section-description">Aplicativos e domínios aparecem somente quando foram capturados no início da gravação.</p>
          <div className="mt-5 border-y border-line"><ApplicationRows items={data.application_details} /></div>
        </div>
        <div className="surface-subtle p-6">
          <div className="flex items-center gap-2 text-[13px] font-medium text-ink"><Clock3 className="h-4 w-4 text-muted" aria-hidden /> Ritmo de uso</div>
          <dl className="mt-5 space-y-5">
            <div><dt className="meta-label">Horário mais ativo</dt><dd className="mt-1 text-[17px] font-semibold">{data.temporal.peak_hour == null ? "Ainda desconhecido" : `${String(data.temporal.peak_hour).padStart(2, "0")}:00–${String((data.temporal.peak_hour + 1) % 24).padStart(2, "0")}:00`}</dd></div>
            <div><dt className="meta-label">Dia mais ativo</dt><dd className="mt-1 text-[14px] font-medium">{data.temporal.peak_weekday == null ? "Ainda desconhecido" : WEEKDAYS[data.temporal.peak_weekday]}</dd></div>
            <div className="grid grid-cols-2 gap-4 border-t border-[#d9d9d3] pt-5"><div><dt className="meta-label">Sequência atual</dt><dd className="mt-1 text-[17px] font-semibold tabular-nums">{data.temporal.current_streak_days} dias</dd></div><div><dt className="meta-label">Maior sequência</dt><dd className="mt-1 text-[17px] font-semibold tabular-nums">{data.temporal.longest_streak_days} dias</dd></div></div>
          </dl>
          <ActivityCalendar activity={data.temporal.activity} />
        </div>
      </section>

      <section className="grid grid-cols-2 gap-8 max-[860px]:grid-cols-1">
        <div>
          <h2 className="section-title">Tipos de uso</h2>
          <p className="section-description">Classificação local e conservadora por app, domínio, content type e profile.</p>
          <div className="mt-4 border-y border-line"><RankedRows items={data.categories} /></div>
        </div>
        <div>
          <h2 className="section-title">Mudanças recentes</h2>
          <p className="section-description">Comparações só aparecem quando os dois períodos possuem amostra suficiente.</p>
          <div className="mt-4 divide-y divide-line border-y border-line">
            {data.trends.length ? data.trends.map((trend) => <TrendRow key={trend.metric} trend={trend} />) : <p className="py-8 text-[13px] text-muted">Continue ditando para formar uma linha de base comparável.</p>}
          </div>
        </div>
      </section>
    </div>
  );
}

function TrendRow({ trend }: { trend: MetricTrend }) {
  const names: Record<string, string> = {
    speaking_speed_wpm: "Velocidade de fala",
    voice_level_lufs: "Nível de voz estimado",
    corrections_per_1000_words: "Correções / 1.000 palavras",
    fillers_per_1000_words: "Palavras de apoio / 1.000",
  };
  const unit = trend.metric === "voice_level_lufs" ? " LU" : trend.metric === "speaking_speed_wpm" ? " PPM" : "";
  return <div className="flex items-center justify-between gap-5 py-3.5"><span className="text-[13px] text-[#41423e]">{names[trend.metric] ?? trend.metric}</span><span className="font-mono text-[11px] tabular-nums text-muted">{number(trend.previous, 1)} → {number(trend.current, 1)}{unit} · {trend.change_percent == null ? signed(trend.change_absolute, unit) : signed(trend.change_percent)}</span></div>;
}

function ConfidenceLabel({ value }: { value: number }) {
  const label = value >= .72 ? "confiança alta" : value >= .48 ? "confiança moderada" : "ainda aprendendo";
  return <span className="font-mono text-[10px] uppercase tracking-[0.08em] text-muted">{label} · {number(value * 100)}%</span>;
}

function TopicDistribution({ topics }: { topics: Array<{ title: string; description: string; share: number }> }) {
  if (!topics.length) return <p className="py-7 text-[13px] leading-5 text-muted">Os temas aparecem quando conceitos relacionados se repetem em mais de uma sessão.</p>;
  return <div className="divide-y divide-line border-y border-line">{topics.slice(0, 5).map((topic) => <div key={topic.title} className="grid grid-cols-[minmax(0,1fr)_minmax(110px,.55fr)_44px] items-center gap-4 py-3.5"><div className="min-w-0"><p className="truncate text-[13px] font-medium text-[#343530]" title={topic.title}>{topic.title}</p><p className="mt-0.5 line-clamp-2 text-[11px] leading-4 text-muted">{topic.description}</p></div><span className="h-1.5 overflow-hidden rounded-full bg-[#e7e7e1]" aria-hidden><span className="block h-full rounded-full bg-[#676862]" style={{ width: `${Math.max(4, topic.share * 100)}%` }} /></span><span className="text-right font-mono text-[11px] tabular-nums text-muted">{number(topic.share * 100)}%</span></div>)}</div>;
}

function GenerationDetails({ data }: { data: InsightsResponse }) {
  const trace = data.profile_generation ?? (data.profile ? {
    generated_at_ms: data.profile.generated_at_ms,
    evidence_bundle: data.profile.evidence_bundle,
    sanitized_prompt: data.profile.sanitized_prompt,
    attempts: data.profile.attempts ?? [],
    schema_validation: data.profile.schema_validation,
  } : null);
  if (!trace) return null;
  return <details className="border-t border-[#d8d8d2] pt-4 text-[11px] text-muted"><summary className="cursor-pointer select-none font-medium text-[#4b4c47]">Inspecionar geração e evidências</summary><div className="mt-4 space-y-4"><p className="font-mono leading-5">schema: {trace.schema_validation} · confidence: {number(trace.evidence_bundle.coverage.overall_confidence * 100)}% · generation: {data.profile?.generation_id ?? "desconhecida"}</p><div><div className="meta-label">Attempts</div><ul className="mt-2 space-y-1.5 font-mono">{trace.attempts.map((attempt, index) => <li key={`${attempt.model}-${index}`} className="break-words">{index + 1}. {attempt.provider} / {attempt.model} · {attempt.status} · total {number(attempt.duration_ms)} ms{attempt.request_ms != null ? ` · request ${number(attempt.request_ms)} ms` : ""}{attempt.ttfb_ms != null ? ` · TTFB ${number(attempt.ttfb_ms)} ms` : ""}{attempt.failure_type ? ` · ${attempt.failure_type}` : ""}{attempt.reported_total_tokens != null ? ` · ${number(attempt.reported_total_tokens)} tokens (${number(attempt.reported_input_tokens ?? 0)} in / ${number(attempt.reported_output_tokens ?? 0)} out)` : ""}{attempt.bytes_sent != null ? ` · ${number(attempt.bytes_sent)} bytes` : ""}{attempt.reported_cost_usd != null ? ` · US$ ${attempt.reported_cost_usd.toFixed(6)} real` : " · custo desconhecido"}{attempt.generation_id ? ` · ${attempt.generation_id}` : ""}{attempt.error ? ` · ${attempt.error}` : ""}</li>)}</ul></div><details><summary className="cursor-pointer">Evidence bundle sanitizado</summary><pre className="mt-2 max-h-80 overflow-auto whitespace-pre-wrap rounded-[8px] bg-[#ecece7] p-3 font-mono text-[10px] leading-4 text-[#4a4b46]">{JSON.stringify(trace.evidence_bundle, null, 2)}</pre></details><details><summary className="cursor-pointer">Prompt sanitizado</summary><pre className="mt-2 max-h-80 overflow-auto whitespace-pre-wrap rounded-[8px] bg-[#ecece7] p-3 font-mono text-[10px] leading-4 text-[#4a4b46]">{trace.sanitized_prompt}</pre></details>{trace.attempts.some((attempt) => attempt.sanitized_response) && <details><summary className="cursor-pointer">Responses sanitizadas</summary><div className="mt-2 space-y-3">{trace.attempts.filter((attempt) => attempt.sanitized_response).map((attempt, index) => <pre key={`${attempt.model}-${index}`} className="max-h-64 overflow-auto whitespace-pre-wrap rounded-[8px] bg-[#ecece7] p-3 font-mono text-[10px] leading-4 text-[#4a4b46]">{attempt.model}\n{attempt.sanitized_response}</pre>)}</div></details>}</div></details>;
}

function VoiceTab({ data, reload, developerMode }: { data: InsightsResponse; reload: () => Promise<void>; developerMode: boolean }) {
  const [profileBusy, setProfileBusy] = useState(false);
  const [profileError, setProfileError] = useState<string | null>(null);
  const [confirmProfile, setConfirmProfile] = useState(false);
  const [vocabBusy, setVocabBusy] = useState(false);
  const [vocabError, setVocabError] = useState<string | null>(null);
  const toggleProfile = async (enabled: boolean) => {
    setProfileError(null);
    try { await setAiVoiceProfileEnabled(enabled); await reload(); } catch (error) { setProfileError(String(error)); }
  };
  useEffect(() => {
    if (!confirmProfile) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    document.getElementById("confirm-voice-profile")?.focus();
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setConfirmProfile(false);
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("keydown", closeOnEscape);
      previous?.focus();
    };
  }, [confirmProfile]);
  const generate = async () => {
    setConfirmProfile(false);
    setProfileBusy(true); setProfileError(null);
    try { await generateAiVoiceProfile(); } catch (error) { setProfileError(String(error)); } finally { await reload(); setProfileBusy(false); }
  };
  const addVocabulary = async () => {
    const correction = data.language.most_corrected;
    if (!correction) return;
    setVocabBusy(true); setVocabError(null);
    try { await addInsightCorrectionToVocabulary(correction.before, correction.after); await reload(); } catch (error) { setVocabError(String(error)); } finally { setVocabBusy(false); }
  };
  const audio = data.audio;
  const profile = data.profile;
  const evidence = data.voice_evidence;
  const hasGeneratedSignature = !!profile && Object.values(profile.signature).some(Boolean);
  const signature = hasGeneratedSignature ? profile.signature : evidence.signature_candidates;
  const communicationPatterns = profile?.communication_patterns.length
    ? profile.communication_patterns.filter((pattern) => pattern.confidence >= .4)
    : evidence.linguistic_patterns.filter((pattern) => pattern.confidence >= .4).slice(0, 4).map((pattern) => ({ title: pattern.title, description: pattern.description, confidence: pattern.confidence, evidence_keys: [pattern.key] }));
  const topics = profile?.recurring_topics.length ? profile.recurring_topics : evidence.recurring_topics;
  const observations = profile?.interesting_observations.filter((observation) => observation.confidence >= .4) ?? [];
  const portraitHabits = profile?.personal_portrait?.distinctive_habits?.filter((habit) => habit.confidence >= .4) ?? [];
  const usageRhythms = profile?.personal_portrait?.usage_rhythms?.filter((rhythm) => rhythm.confidence >= .4) ?? [];
  const suggestedExperiments = profile?.suggested_experiments?.filter((experiment) => experiment.confidence >= .4) ?? [];
  const remainingProfileWords = voiceProfileRemainingWords(data.profile_progress_words, data.profile_required_words);
  const lufsTrend = data.trends.find((trend) => trend.metric === "voice_level_lufs");
  return (
    <div className="space-y-10">
      <section className="surface overflow-hidden">
        <div className="grid grid-cols-[1fr_290px] max-[900px]:grid-cols-1">
          <div className="relative overflow-hidden p-8 pr-12 max-[700px]:p-6">
            <div className="absolute -right-16 -top-24 h-56 w-56 rounded-full border border-[#e2e2dc]" aria-hidden />
            <div className="relative">
              <div className="flex items-center gap-2 text-[12px] font-medium uppercase tracking-[0.08em] text-muted"><Sparkles className="h-4 w-4" aria-hidden /> Seu archetype de ditado</div>
              {profile ? <><div className="mt-7 flex flex-wrap items-end gap-x-4 gap-y-2"><h2 className="text-[38px] font-semibold leading-none tracking-[-0.045em] text-ink">{profile.archetype.title || profile.title}</h2><ConfidenceLabel value={profile.archetype.confidence} /></div>{profile.archetype.subtitle && <p className="mt-3 text-[15px] font-medium text-[#4d4e49]">{profile.archetype.subtitle}</p>}<p className="mt-4 max-w-[68ch] text-[14px] leading-6 text-[#555650]">{profile.archetype.description || profile.description}</p><div className="mt-7 max-w-lg"><div className="mb-2 flex items-center justify-between text-[11px] text-muted"><span>{data.profile_generation_ready ? "Novo nível liberado" : "Seu perfil está ficando mais preciso"}</span><span className="tabular-nums">{number(data.profile_progress_words)} / {number(data.profile_required_words)}</span></div><div className="h-1.5 overflow-hidden rounded-full bg-[#e7e7e1]"><span className="block h-full rounded-full bg-[#555650]" style={{ width: `${voiceProfileProgress(data.profile_progress_words, data.profile_required_words)}%` }} /></div></div></> : data.profile_generation_ready ? <><h2 className="mt-7 text-[28px] font-semibold tracking-[-0.035em]">Seu primeiro perfil está pronto para nascer</h2><p className="mt-3 max-w-[62ch] text-[13px] leading-5 text-muted">A interpretação conecta temas, intenções e padrões calculados localmente. Nenhuma gravação ou histórico completo é enviado.</p></> : <><h2 className="mt-7 text-[26px] font-semibold tracking-[-0.035em]">Sua voz ainda está ganhando forma</h2><p className="mt-3 max-w-[62ch] text-[13px] leading-5 text-muted">As estatísticas locais já funcionam. Com 500 palavras, o Haumea libera o primeiro perfil; depois ele evolui nos marcos de 2.000, 5.000 e 10.000 palavras.</p><div className="mt-6 max-w-md"><div className="h-1.5 overflow-hidden rounded-full bg-[#e7e7e1]"><span className="block h-full rounded-full bg-[#555650]" style={{ width: `${voiceProfileProgress(data.profile_progress_words, data.profile_required_words)}%` }} /></div><p className="mt-2 text-[11px] tabular-nums text-muted">Próximo nível · {number(data.profile_progress_words)} / {number(data.profile_required_words)} palavras</p></div></>}
              {developerMode && <div className="mt-6"><GenerationDetails data={data} /></div>}
            </div>
          </div>
          <div className="border-l border-line bg-[#f3f3ef] p-6 max-[900px]:border-l-0 max-[900px]:border-t">
            <div className="flex items-center justify-between gap-4"><div><div className="text-[13px] font-medium">Perfil gerado por IA</div><p className="mt-1 text-[11px] leading-4 text-muted">Opcional. Estatísticas locais permanecem ativas quando desligado.</p></div><Toggle checked={data.profile_enabled} onChange={toggleProfile} label="Ativar Voice Profile por IA" /></div>
            <Button className="mt-5 w-full" variant="primary" disabled={!data.profile_enabled || !data.profile_generation_ready || profileBusy} onClick={() => setConfirmProfile(true)}>{profileBusy ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}{profileBusy ? "Gerando perfil" : data.profile_generation_ready ? (profile ? "Atualizar perfil" : "Gerar perfil") : `Próximo nível em ${number(remainingProfileWords)} palavras`}</Button>
            <p className="mt-3 text-[11px] leading-4 text-muted">Chamada externa: OpenRouter · meta/muse-spark-1.2-contributor. Sem fallback. Cooldown de 1 minuto.</p>
          </div>
        </div>
      </section>
      {confirmProfile && <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/20 p-5" role="presentation" onMouseDown={(event) => { if (event.currentTarget === event.target) setConfirmProfile(false); }}><div role="dialog" aria-modal="true" aria-labelledby="voice-profile-confirm-title" className="surface w-full max-w-md p-6 shadow-xl"><h2 id="voice-profile-confirm-title" className="text-[17px] font-semibold text-ink">Gerar Voice Profile com IA?</h2><p className="mt-3 text-[13px] leading-5 text-muted">O Haumea enviará métricas agregadas de uso, contexto, ritmo, linguagem e áudio, além de termos filtrados. Nenhuma transcrição completa ou gravação será enviada. A geração usa exclusivamente <span className="font-mono text-[11px] text-[#444540]">meta/muse-spark-1.2-contributor</span> via OpenRouter, sem fallback para outro modelo.</p><div className="mt-6 flex justify-end gap-2"><Button onClick={() => setConfirmProfile(false)}>Cancelar</Button><Button id="confirm-voice-profile" variant="primary" onClick={generate}>Gerar perfil</Button></div></div></div>}
      {profileError && <ErrorState>{profileError}</ErrorState>}

      {profile?.personal_portrait?.summary && <section>
        <div className="flex items-end justify-between gap-5"><div><h2 className="section-title">Um retrato do seu ditado</h2><p className="section-description">Uma leitura pessoal, mas limitada ao que seus padrões de uso realmente sustentam.</p></div><ConfidenceLabel value={profile.personal_portrait.confidence} /></div>
        <div className="mt-5 border-y border-line py-7"><p className="max-w-[76ch] whitespace-pre-line text-[15px] leading-7 text-[#41423d]">{profile.personal_portrait.summary}</p></div>
        {(portraitHabits.length > 0 || usageRhythms.length > 0) && <div className="mt-7 grid grid-cols-2 gap-10 max-[820px]:grid-cols-1">
          {portraitHabits.length > 0 && <ProfileInsightList title="O que torna seu uso distintivo" items={portraitHabits} developerMode={developerMode} />}
          {usageRhythms.length > 0 && <ProfileInsightList title="Seu ritmo de uso" items={usageRhythms} developerMode={developerMode} />}
        </div>}
      </section>}

      <section>
        <div className="flex items-end justify-between gap-5"><div><h2 className="section-title">Sua assinatura</h2><p className="section-description">Candidatos extraídos literalmente de várias sessões; a IA pode selecionar, mas não inventar.</p></div><span className="shrink-0 font-mono text-[10px] uppercase tracking-[0.08em] text-muted">{voiceEvidenceLevelLabel(evidence.coverage.level)}</span></div>
        <div className="mt-5 grid grid-cols-[1.25fr_repeat(3,minmax(0,1fr))] border-y border-line max-[920px]:grid-cols-2 max-[560px]:grid-cols-1">
          <div className="border-r border-line py-6 pr-6 max-[920px]:border-b max-[560px]:border-r-0"><div className="meta-label">Catchphrase</div><blockquote className="mt-3 text-[27px] font-semibold leading-tight tracking-[-0.035em] text-ink">{signature.catchphrase ? `“${signature.catchphrase}”` : "Ainda sem padrão estável"}</blockquote></div>
          <SignatureFact label="Abertura típica" value={signature.opener} />
          <SignatureFact label="Conector favorito" value={signature.connector} />
          <SignatureFact label="Palavra característica" value={signature.content_word} />
        </div>
      </section>

      <section className="grid grid-cols-[minmax(0,1.05fr)_minmax(320px,.95fr)] gap-10 max-[900px]:grid-cols-1">
        <div><h2 className="section-title">Como você estrutura o ditado</h2><p className="section-description">Padrões observáveis — nunca traços de personalidade.</p><div className="mt-5 divide-y divide-line border-y border-line">{communicationPatterns.length ? communicationPatterns.map((pattern) => <article key={pattern.title} className="py-5"><div className="flex items-start justify-between gap-5"><h3 className="text-[14px] font-semibold text-[#343530]">{pattern.title}</h3><ConfidenceLabel value={pattern.confidence} /></div><p className="mt-2 text-[13px] leading-5 text-muted">{pattern.description}</p>{developerMode && <p className="mt-2 break-all font-mono text-[10px] text-[#85867f]">{pattern.evidence_keys.join(" · ")}</p>}</article>) : <p className="py-8 text-[13px] text-muted">Ainda estamos aprendendo quais padrões se repetem entre sessões.</p>}</div></div>
        <div><h2 className="section-title">Temas recorrentes</h2><p className="section-description">Conceitos relacionados são agrupados incrementalmente, sem enviar o histórico à nuvem.</p><div className="mt-5"><TopicDistribution topics={topics} /></div>{evidence.recurring_intents.length > 0 && <div className="mt-8"><h3 className="text-[13px] font-semibold text-[#343530]">Para que você dita</h3><p className="mt-1 text-[11px] leading-4 text-muted">Distribuição conservadora da intenção principal por sessão.</p><div className="mt-3 divide-y divide-line border-y border-line">{evidence.recurring_intents.slice(0, 4).map((intent) => <div key={intent.title} className="flex items-center justify-between gap-5 py-3"><span className="text-[13px] text-[#4a4b46]">{intent.title}</span><span className="font-mono text-[11px] tabular-nums text-muted">{number(intent.share * 100)}%</span></div>)}</div></div>}</div>
      </section>

      {observations.length > 0 && <section><h2 className="section-title">Observações interessantes</h2><p className="section-description">Conexões interpretativas apoiadas nas evidências do período.</p><div className="mt-5 grid grid-cols-2 gap-x-9 border-y border-line max-[800px]:grid-cols-1">{observations.slice(0, 6).map((observation) => <article key={`${observation.type}-${observation.title}`} className="border-b border-line py-5 last:border-b-0"><div className="flex items-center justify-between gap-4"><span className="font-mono text-[10px] uppercase tracking-[0.08em] text-muted">{observation.type}</span><ConfidenceLabel value={observation.confidence} /></div><h3 className="mt-3 text-[14px] font-semibold text-[#343530]">{observation.title}</h3><p className="mt-2 text-[13px] leading-5 text-muted">{observation.description}</p>{developerMode && <p className="mt-2 break-all font-mono text-[10px] text-[#85867f]">{observation.evidence_keys.join(" · ")}</p>}</article>)}</div></section>}

      {suggestedExperiments.length > 0 && <section><h2 className="section-title">Experimentos para o seu fluxo</h2><p className="section-description">Sugestões opcionais geradas a partir dos seus padrões — não regras genéricas de produtividade.</p><div className="mt-5 grid grid-cols-3 border-y border-line max-[900px]:grid-cols-1">{suggestedExperiments.map((experiment) => <article key={experiment.title} className="border-r border-line px-6 py-6 last:border-r-0 max-[900px]:border-b max-[900px]:border-r-0"><div className="flex items-start justify-between gap-4"><h3 className="text-[14px] font-semibold text-[#343530]">{experiment.title}</h3><ConfidenceLabel value={experiment.confidence} /></div><p className="mt-3 text-[13px] leading-5 text-muted">{experiment.description}</p>{developerMode && <p className="mt-3 break-all font-mono text-[10px] text-[#85867f]">{experiment.evidence_keys.join(" · ")}</p>}</article>)}</div></section>}

      <section className="grid grid-cols-[1.15fr_.85fr] gap-8 max-[900px]:grid-cols-1">
        <div className="border-y border-line py-7"><div className="meta-label">Expressão significativa mais frequente</div>{signature.phrase || data.language.most_used_phrase ? <blockquote className="mt-4 max-w-[28ch] text-[28px] font-semibold leading-tight tracking-[-0.035em] text-ink">“{signature.phrase ?? data.language.most_used_phrase?.label}”</blockquote> : <p className="mt-4 text-[17px] font-medium text-[#4d4e49]">Ainda não há uma expressão estável.</p>}<p className="mt-3 text-[13px] text-muted">Separada da catchphrase: aqui vence a frequência significativa entre sessões.</p></div>
        <dl className="divide-y divide-line border-y border-line"><VoiceFact label="Palavra literal mais usada" value={data.language.most_used_word?.label} detail="Inclui stopwords" /><div className="py-4"><dt className="meta-label">Mais corrigido</dt>{data.language.most_corrected ? <dd className="mt-2"><div className="flex items-center gap-2 text-[14px]"><span className="text-muted line-through">{data.language.most_corrected.before}</span><span>→</span><strong>{data.language.most_corrected.after}</strong></div><div className="mt-2 flex items-center justify-between gap-3"><span className="text-[11px] text-muted">{number(data.language.most_corrected.count)} correções</span>{data.language.most_corrected.in_vocabulary ? <span className="inline-flex items-center gap-1 text-[11px] text-[#25613f]"><Check className="h-3.5 w-3.5" /> No vocabulário</span> : <Button size="sm" disabled={vocabBusy} onClick={addVocabulary}>{vocabBusy ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : null}Adicionar ao vocabulário</Button>}</div></dd> : <dd className="mt-2 text-[13px] text-muted">Nenhuma correção lexical recorrente.</dd>}</div></dl>
      </section>
      {vocabError && <ErrorState>{vocabError}</ErrorState>}

      <section>
        <div className="flex items-end justify-between gap-6"><div><h2 className="section-title">Como sua captura soa</h2><p className="section-description">Medições locais do sinal. Volume baixo e qualidade ruim são tratados como coisas diferentes.</p></div><span className="shrink-0 font-mono text-[11px] text-muted">Cobertura de áudio {number(audio.coverage_percentage, 0)}%</span></div>
        {audio.analyzed_sessions ? <div className="mt-5 surface overflow-hidden"><div className="grid grid-cols-[1.25fr_.75fr_.75fr] max-[900px]:grid-cols-1"><div className="border-r border-line p-7 max-[900px]:border-b max-[900px]:border-r-0"><div className="flex items-center gap-1.5 text-[13px] text-muted"><Volume2 className="h-4 w-4" /> Nível de voz <MetricHelp label="Estimativa local de loudness integrado para fala mono. Não é medição laboratorial calibrada." /></div><div className="mt-4 flex items-baseline gap-2"><strong className="text-[42px] font-semibold tracking-[-0.04em] tabular-nums">{audio.lufs_median == null ? "—" : number(audio.lufs_median, 1)}</strong><span className="text-[13px] text-muted">LUFS estimado</span></div><p className="mt-2 text-[13px] text-muted">{audio.lufs_median != null && audio.lufs_median < -28 ? "Sua voz costuma ser capturada em volume mais baixo." : "Seu nível de captura costuma ficar em uma faixa estável."}</p><div className="mt-4"><TrendBadge trend={lufsTrend} absolute /></div>{audio.lufs_typical && <p className="mt-4 font-mono text-[11px] text-muted">Faixa típica {number(audio.lufs_typical[0], 1)} → {number(audio.lufs_typical[1], 1)} LUFS</p>}</div><div className="border-r border-line p-7 max-[900px]:border-b max-[900px]:border-r-0"><div className="text-[13px] text-muted">Qualidade de captura</div><div className="mt-4 text-[26px] font-semibold tracking-[-0.03em]">{audio.capture_quality ?? "—"}</div><p className="mt-2 text-[11px] leading-4 text-muted">Derivada de clipping, SNR estimado, loudness e detectabilidade de fala.</p></div><div className="p-7"><div className="text-[13px] text-muted">Variação de pitch</div><div className="mt-4 text-[26px] font-semibold tracking-[-0.03em]">{audio.pitch_variation ?? "—"}</div><p className="mt-2 text-[11px] leading-4 text-muted">Descrição acústica da variação de F0; não infere emoção ou personalidade.</p></div></div>
          <details className="border-t border-line px-7 py-4"><summary className="cursor-pointer text-[13px] font-medium text-[#4b4c47]">Detalhes avançados de áudio</summary><dl className="mt-5 grid grid-cols-3 gap-x-8 gap-y-5 max-[800px]:grid-cols-2"><AudioFact label="RMS" value={audio.rms_dbfs_median == null ? null : `${number(audio.rms_dbfs_median, 1)} dBFS`} /><AudioFact label="Peak" value={audio.peak_dbfs_median == null ? null : `${number(audio.peak_dbfs_median, 1)} dBFS`} /><AudioFact label="SNR estimado" value={audio.estimated_snr_db == null ? null : `${number(audio.estimated_snr_db, 1)} dB`} /><AudioFact label="Fala / silêncio" value={audio.speech_ratio == null ? null : `${number(audio.speech_ratio * 100)}% / ${number((audio.silence_ratio ?? 0) * 100)}%`} /><AudioFact label="Pausa média" value={audio.average_pause_ms == null ? null : `${number(audio.average_pause_ms)} ms`} /><AudioFact label="F0 mediana" value={audio.median_f0_hz == null ? null : `${number(audio.median_f0_hz)} Hz`} /><AudioFact label="Clipping" value={audio.clipping_ratio == null ? null : `${number(audio.clipping_ratio * 100, 2)}%`} /></dl></details></div> : <div className="mt-5 surface px-7 py-10"><AudioLines className="h-5 w-5 text-muted" /><p className="mt-4 text-[14px] font-medium">Métricas acústicas ainda indisponíveis</p><p className="mt-1 max-w-xl text-[13px] leading-5 text-muted">O histórico deste período não possui WAV compatível analisado. Insights linguísticos continuam funcionando normalmente.</p></div>}
      </section>

      <section className="grid grid-cols-2 gap-8 max-[860px]:grid-cols-1">
        <div><h2 className="section-title">Palavras de apoio</h2><p className="section-description">Ocorrências descritivas por 1.000 palavras; não são tratadas como erro.</p><div className="mt-4 divide-y divide-line border-y border-line">{data.language.fillers.length ? data.language.fillers.map((item) => <div key={item.phrase} className="flex items-center justify-between py-3.5"><span className="text-[13px]">“{item.phrase}”</span><span className="font-mono text-[11px] tabular-nums text-muted">{number(item.per_1000_words, 1)} / 1k</span></div>) : <p className="py-8 text-[13px] text-muted">Nenhuma palavra de apoio recorrente detectada.</p>}</div></div>
        <div><h2 className="section-title">Hábitos linguísticos</h2><p className="section-description">Métricas calculadas localmente sobre transcrições e correções explícitas.</p><dl className="mt-4 divide-y divide-line border-y border-line"><VoiceFact label="Autocorreções" value={data.language.self_corrections_per_1000_words == null ? undefined : `${number(data.language.self_corrections_per_1000_words, 1)} / 1.000 palavras`} detail="Padrões explícitos e estágios de Backtrack" /><VoiceFact label="Variedade de vocabulário" value={data.language.vocabulary_variety_label ?? undefined} detail={data.language.vocabulary_variety == null ? "Amostra insuficiente" : `MATTR ${number(data.language.vocabulary_variety, 3)} · janelas de 50 tokens`} /><VoiceFact label="Velocidade típica" value={data.usage.typical_wpm ? `${number(data.usage.typical_wpm[0])}–${number(data.usage.typical_wpm[1])} PPM` : undefined} detail="Percentis 10–90 sobre sessões com duração de fala" /></dl></div>
      </section>
    </div>
  );
}

function ProfileInsightList({ title, items, developerMode }: { title: string; items: Array<{ title: string; description: string; confidence: number; evidence_keys: string[] }>; developerMode: boolean }) {
  return <div><h3 className="text-[13px] font-semibold text-[#343530]">{title}</h3><div className="mt-3 divide-y divide-line border-y border-line">{items.map((item) => <article key={item.title} className="py-4"><div className="flex items-start justify-between gap-4"><h4 className="text-[13px] font-medium text-[#444540]">{item.title}</h4><ConfidenceLabel value={item.confidence} /></div><p className="mt-2 text-[12px] leading-5 text-muted">{item.description}</p>{developerMode && <p className="mt-2 break-all font-mono text-[10px] text-[#85867f]">{item.evidence_keys.join(" · ")}</p>}</article>)}</div></div>;
}

function SignatureFact({ label, value }: { label: string; value?: string | null }) {
  return <div className="border-r border-line px-5 py-6 last:border-r-0 max-[920px]:border-b max-[560px]:border-r-0"><div className="meta-label">{label}</div><p className="mt-3 break-words text-[16px] font-semibold leading-5 text-[#343530]">{value ? `“${value}”` : "Ainda aprendendo"}</p></div>;
}

function VoiceFact({ label, value, detail }: { label: string; value?: string; detail?: string }) {
  return <div className="py-4"><dt className="meta-label">{label}</dt><dd className="mt-1.5 text-[14px] font-medium text-[#343530]">{value ?? "Ainda não há dados suficientes"}{detail && <span className="mt-1 block text-[11px] font-normal text-muted">{detail}</span>}</dd></div>;
}

function AudioFact({ label, value }: { label: string; value: string | null }) {
  return <div><dt className="meta-label">{label}</dt><dd className="mt-1 font-mono text-[11px] tabular-nums text-[#444540]">{value ?? "Desconhecido"}</dd></div>;
}

export function InsightsView() {
  const [tab, setTab] = useState<Tab>("usage");
  const [period, setPeriod] = useState<InsightPeriod>("last30_days");
  const [data, setData] = useState<InsightsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [backfillBusy, setBackfillBusy] = useState(false);
  const [backfillError, setBackfillError] = useState<string | null>(null);
  const [developerMode, setDeveloperMode] = useState(false);
  const mountedRef = useRef(true);
  const periodRef = useRef(period);
  const reloadRunningRef = useRef(false);
  const reloadQueuedRef = useRef(false);
  const reload = useCallback(async () => {
    if (reloadRunningRef.current) {
      reloadQueuedRef.current = true;
      return;
    }
    reloadRunningRef.current = true;
    try {
      do {
        reloadQueuedRef.current = false;
        const requestedPeriod = periodRef.current;
        if (mountedRef.current) setError(null);
        try {
          const response = await getInsights(requestedPeriod);
          if (mountedRef.current && requestedPeriod === periodRef.current) setData(response);
        } catch (reason) {
          if (mountedRef.current) setError(String(reason));
        } finally {
          if (mountedRef.current) setLoading(false);
        }
      } while (reloadQueuedRef.current && mountedRef.current);
    } finally {
      reloadRunningRef.current = false;
    }
  }, []);
  useEffect(() => {
    mountedRef.current = true;
    void getDevMode().then((enabled) => { if (mountedRef.current) setDeveloperMode(enabled); }).catch(() => undefined);
    return () => { mountedRef.current = false; };
  }, []);
  useEffect(() => {
    periodRef.current = period;
    setLoading(true);
    void reload();
  }, [period, reload]);
  useEffect(() => {
    let disposed = false;
    const unlisten: Array<() => void> = [];
    void Promise.all([
      listen("insights-progress", () => void reload()),
      listen("insights-updated", () => void reload()),
    ]).then((listeners) => {
      if (disposed) listeners.forEach((listener) => listener());
      else unlisten.push(...listeners);
    });
    return () => { disposed = true; unlisten.forEach((listener) => listener()); };
  }, [reload]);
  const pauseBackfill = async () => {
    if (!data) return;
    setBackfillBusy(true); setBackfillError(null);
    try { await setInsightsBackfillPaused(!data.backfill.paused); await reload(); } catch (reason) { setBackfillError(String(reason)); } finally { setBackfillBusy(false); }
  };
  const switchTab = (next: Tab) => {
    setTab(next);
    requestAnimationFrame(() => document.getElementById(`insights-tab-${next}`)?.focus());
  };
  const handleTabKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight" && event.key !== "Home" && event.key !== "End") return;
    event.preventDefault();
    switchTab(adjacentInsightsTab(tab, event.key));
  };
  const hasData = !!data?.usage.sessions;
  return (
    <div>
      <PageHeader title="Insights" description="Uma leitura local de como você usa ditado, como costuma falar e como sua captura de voz se comporta." action={<div className="inline-flex rounded-[10px] border border-line bg-white p-1" aria-label="Período analisado">{PERIODS.map((item) => <button key={item.value} type="button" onClick={() => setPeriod(item.value)} aria-pressed={period === item.value} className={`h-8 rounded-[7px] px-3 text-[11px] font-medium transition-colors ${period === item.value ? "bg-[#242422] text-white" : "text-muted hover:bg-[#f0f0eb] hover:text-ink"}`}>{item.label}</button>)}</div>} />
      <div className="mb-8 flex border-b border-line" role="tablist" aria-label="Áreas de Insights"><button id="insights-tab-usage" type="button" role="tab" aria-selected={tab === "usage"} aria-controls="insights-panel-usage" tabIndex={tab === "usage" ? 0 : -1} onKeyDown={handleTabKeyDown} onClick={() => setTab("usage")} className={`relative px-1 pb-3 pr-6 text-[13px] font-medium ${tab === "usage" ? "text-ink after:absolute after:inset-x-0 after:bottom-[-1px] after:h-px after:bg-ink" : "text-muted"}`}>Seu uso</button><button id="insights-tab-voice" type="button" role="tab" aria-selected={tab === "voice"} aria-controls="insights-panel-voice" tabIndex={tab === "voice" ? 0 : -1} onKeyDown={handleTabKeyDown} onClick={() => setTab("voice")} className={`relative px-6 pb-3 text-[13px] font-medium ${tab === "voice" ? "text-ink after:absolute after:inset-x-5 after:bottom-[-1px] after:h-px after:bg-ink" : "text-muted"}`}>Sua voz</button></div>
      {data?.backfill.running && <div className="mb-7 flex items-center justify-between gap-5 rounded-[10px] bg-[#eeeeea] px-4 py-3" role="status" aria-live="polite"><div className="flex min-w-0 items-center gap-3">{data.backfill.paused ? <Pause className="h-4 w-4 text-muted" /> : <Loader2 className="h-4 w-4 animate-spin text-muted" />}<div className="min-w-0"><p className="text-[13px] font-medium text-[#42433f]">{data.backfill.paused ? "Análise do histórico pausada" : "Analisando seu histórico…"}</p><p className="mt-0.5 text-[11px] tabular-nums text-muted">{number(data.backfill.processed)} / {number(data.backfill.total)} gravações · retomada automática após reiniciar</p></div></div><Button size="sm" variant="ghost" disabled={backfillBusy} onClick={pauseBackfill}>{backfillBusy ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : data.backfill.paused ? <Play className="h-3.5 w-3.5" /> : <Pause className="h-3.5 w-3.5" />}{data.backfill.paused ? "Continuar" : "Pausar"}</Button></div>}
      {backfillError && <ErrorState>{backfillError}</ErrorState>}
      {error && <ErrorState>{error}</ErrorState>}
      {loading && !data ? <div className="surface"><SkeletonRows count={5} /></div> : !hasData ? <div className="surface flex min-h-[360px] flex-col items-center justify-center px-8 text-center"><Activity className="h-6 w-6 text-[#8b8c85]" /><h2 className="mt-5 text-[17px] font-semibold">Seus Insights começam com o próximo ditado</h2><p className="mt-2 max-w-lg text-[13px] leading-5 text-muted">O Haumea analisará localmente palavras, horários e áudio disponível. O histórico existente é processado em background sem bloquear a transcrição.</p></div> : data && <div id={`insights-panel-${tab}`} role="tabpanel" aria-labelledby={`insights-tab-${tab}`} tabIndex={0}>{tab === "usage" ? <UsageTab data={data} /> : <VoiceTab data={data} reload={reload} developerMode={developerMode} />}</div>}
      {data && <footer className="mt-12 flex items-center justify-between border-t border-line pt-5 text-[11px] text-muted">{developerMode ? <span>Analysis version {data.analysis_version} · {number(data.audio.analyzed_sessions)} gravações com áudio analisado</span> : <span>{number(data.audio.analyzed_sessions)} gravações com áudio analisado</span>}<span className="inline-flex items-center gap-1.5"><BrainCircuit className="h-3.5 w-3.5" /> Estatísticas calculadas localmente</span></footer>}
    </div>
  );
}
