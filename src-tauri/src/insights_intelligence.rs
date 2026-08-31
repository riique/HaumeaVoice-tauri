//! Deterministic evidence extraction for Voice Insights.
//!
//! This module never persists raw transcript text. It turns one session into
//! compact semantic tags and calculable communication signals that can be
//! incrementally aggregated by `insights`.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const BASIC_WORDS: u64 = 500;
pub const ARCHETYPE_WORDS: u64 = 2_000;
pub const RICH_WORDS: u64 = 5_000;
pub const HIGH_CONFIDENCE_WORDS: u64 = 10_000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SemanticContribution {
    pub topics: BTreeMap<String, f64>,
    pub intent: String,
    pub intent_confidence: f64,
    pub patterns: BTreeSet<String>,
    pub connector_counts: BTreeMap<String, u64>,
    pub opener: Option<String>,
    pub question_count: u64,
    pub example_count: u64,
    pub requirement_addition_count: u64,
    pub sentence_count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvidenceItem {
    pub key: String,
    pub title: String,
    pub description: String,
    pub count: u64,
    pub share: f64,
    pub confidence: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SignatureEvidence {
    pub catchphrase: Option<String>,
    pub content_word: Option<String>,
    pub phrase: Option<String>,
    pub connector: Option<String>,
    pub opener: Option<String>,
    pub corrected_expression: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvidenceStatistics {
    pub sessions: u64,
    pub words: u64,
    #[serde(default)]
    pub average_words_per_session: Option<f64>,
    #[serde(default)]
    pub average_duration_seconds: Option<f64>,
    pub average_wpm: Option<f64>,
    #[serde(default)]
    pub typical_wpm: Option<[f64; 2]>,
    pub manual_corrections: u64,
    pub self_corrections_per_1000_words: Option<f64>,
    pub vocabulary_variety_mattr: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvidenceCoverage {
    pub level: String,
    pub overall_confidence: f64,
    pub session_coverage: f64,
    pub audio_coverage: f64,
    pub words: u64,
    pub sessions: u64,
    pub next_level_words: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoiceProfileEvidence {
    pub statistics: EvidenceStatistics,
    pub recurring_topics: Vec<EvidenceItem>,
    pub recurring_intents: Vec<EvidenceItem>,
    pub linguistic_patterns: Vec<EvidenceItem>,
    pub signature_candidates: SignatureEvidence,
    pub correction_patterns: Vec<EvidenceItem>,
    pub application_patterns: Vec<EvidenceItem>,
    #[serde(default)]
    pub workflow_patterns: Vec<EvidenceItem>,
    pub acoustic_patterns: Vec<EvidenceItem>,
    pub temporal_patterns: Vec<EvidenceItem>,
    pub trends: Vec<EvidenceItem>,
    pub coverage: EvidenceCoverage,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArchetypeProfile {
    pub title: String,
    pub subtitle: String,
    pub description: String,
    pub confidence: f64,
    pub evidence_keys: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SignatureProfile {
    pub catchphrase: Option<String>,
    pub content_word: Option<String>,
    pub phrase: Option<String>,
    pub connector: Option<String>,
    pub opener: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InterpretedPattern {
    pub title: String,
    pub description: String,
    pub confidence: f64,
    pub evidence_keys: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileTopic {
    pub title: String,
    pub description: String,
    pub share: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationType {
    #[default]
    Language,
    Usage,
    Acoustic,
    Temporal,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InterestingObservation {
    pub title: String,
    pub description: String,
    #[serde(rename = "type")]
    pub observation_type: ObservationType,
    pub confidence: f64,
    pub evidence_keys: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersonalPortrait {
    pub summary: String,
    pub confidence: f64,
    pub evidence_keys: Vec<String>,
    #[serde(default)]
    pub distinctive_habits: Vec<InterpretedPattern>,
    #[serde(default)]
    pub usage_rhythms: Vec<InterpretedPattern>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeneratedVoiceProfile {
    pub archetype: ArchetypeProfile,
    #[serde(default)]
    pub personal_portrait: PersonalPortrait,
    pub signature: SignatureProfile,
    pub communication_patterns: Vec<InterpretedPattern>,
    pub recurring_topics: Vec<ProfileTopic>,
    pub interesting_observations: Vec<InterestingObservation>,
    #[serde(default)]
    pub suggested_experiments: Vec<InterpretedPattern>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileValidationFailure {
    InvalidJson,
    SchemaValidation(String),
}

impl ProfileValidationFailure {
    pub fn failure_type(&self) -> &'static str {
        match self {
            Self::InvalidJson | Self::SchemaValidation(_) => "schema_validation",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidJson => "resposta não contém JSON válido".into(),
            Self::SchemaValidation(message) => message.clone(),
        }
    }
}

struct TopicRule {
    key: &'static str,
    title: &'static str,
    keywords: &'static [&'static str],
}

const TOPICS: &[TopicRule] = &[
    TopicRule {
        key: "voice_ai",
        title: "Voice AI & desenvolvimento",
        keywords: &[
            "openrouter",
            "gemini",
            "deepgram",
            "whisper",
            "transcrição",
            "transcription",
            "stt",
            "provider",
            "pipeline",
            "áudio",
            "audio",
            "speech",
            "microfone",
        ],
    },
    TopicRule {
        key: "software",
        title: "Desenvolvimento de software",
        keywords: &[
            "código",
            "codigo",
            "rust",
            "react",
            "typescript",
            "javascript",
            "api",
            "função",
            "funcao",
            "bug",
            "build",
            "teste",
            "testes",
            "git",
            "github",
            "backend",
            "frontend",
            "banco",
            "database",
        ],
    },
    TopicRule {
        key: "biology",
        title: "Biologia & ciências da vida",
        keywords: &[
            "dna",
            "rna",
            "cromossomo",
            "cromossomos",
            "mitose",
            "meiose",
            "célula",
            "celula",
            "genética",
            "genetica",
            "biologia",
            "proteína",
            "proteina",
            "organismo",
            "gene",
            "genes",
        ],
    },
    TopicRule {
        key: "study",
        title: "Estudo & aprendizagem",
        keywords: &[
            "estudo",
            "estudar",
            "prova",
            "aula",
            "resumo",
            "exercício",
            "exercicio",
            "questão",
            "questao",
            "conceito",
            "explica",
            "explique",
            "aprender",
            "revisão",
            "revisao",
        ],
    },
    TopicRule {
        key: "product",
        title: "Produto & experiência",
        keywords: &[
            "produto",
            "feature",
            "funcionalidade",
            "usuário",
            "usuario",
            "design",
            "interface",
            "ux",
            "ui",
            "roadmap",
            "requisito",
            "experiência",
            "experiencia",
            "fluxo",
        ],
    },
    TopicRule {
        key: "writing",
        title: "Escrita & comunicação",
        keywords: &[
            "texto",
            "mensagem",
            "email",
            "e-mail",
            "escrever",
            "redigir",
            "revisar",
            "documento",
            "assinatura",
            "resposta",
            "parágrafo",
            "paragrafo",
        ],
    },
    TopicRule {
        key: "planning",
        title: "Planejamento & organização",
        keywords: &[
            "plano",
            "planejar",
            "etapa",
            "etapas",
            "tarefa",
            "agenda",
            "organizar",
            "prioridade",
            "cronograma",
            "objetivo",
            "próximo",
            "proximo",
        ],
    },
];

pub fn analyze_semantics(
    text: &str,
    tokens: &[String],
    category: &str,
    app: Option<&str>,
    domain: Option<&str>,
    self_corrections: u64,
) -> SemanticContribution {
    let token_set: BTreeSet<_> = tokens.iter().map(String::as_str).collect();
    let lower = text.to_lowercase();
    let mut topic_scores = Vec::new();
    for rule in TOPICS {
        let matches = rule
            .keywords
            .iter()
            .filter(|keyword| {
                token_set.contains(**keyword) || count_bounded_markers(&lower, keyword) > 0
            })
            .count() as f64;
        let context_boost = topic_context_boost(rule.key, category, app, domain);
        let score = matches + context_boost;
        if score >= 2.0 {
            topic_scores.push((rule.key.to_string(), score));
        }
    }
    topic_scores.sort_by(|left, right| right.1.total_cmp(&left.1));
    topic_scores.truncate(2);
    let total_topic_score: f64 = topic_scores.iter().map(|(_, score)| *score).sum();
    let topics = topic_scores
        .into_iter()
        .map(|(key, score)| (key, score / total_topic_score.max(1.0)))
        .collect();

    let (intent, intent_confidence) = detect_intent(&lower, tokens, category);
    let question_count = text.matches('?').count() as u64
        + count_markers(&lower, &["como ", "por que ", "o que ", "qual ", "quais "]);
    let example_count = count_markers(
        &lower,
        &[
            "por exemplo",
            "exemplo",
            "ex:",
            "como quando",
            "for example",
        ],
    );
    let requirement_addition_count = count_markers(
        &lower,
        &[
            "além disso",
            "alem disso",
            "também",
            "tambem",
            "e também",
            "outro requisito",
            "mais uma coisa",
            "adicionalmente",
        ],
    );
    let sentence_count = text
        .chars()
        .filter(|character| matches!(character, '.' | '!' | '?'))
        .count()
        .max(1) as u64;
    let mut patterns = BTreeSet::new();
    if self_corrections > 0
        || contains_any(
            &lower,
            &["quer dizer", "corrigindo", "na verdade", "melhor"],
        )
    {
        patterns.insert("iterative_refinement".into());
    }
    if example_count > 0 {
        patterns.insert("uses_examples".into());
    }
    if requirement_addition_count >= 2 || (requirement_addition_count > 0 && tokens.len() >= 50) {
        patterns.insert("adds_requirements".into());
    }
    if question_count > 0 {
        patterns.insert("asks_questions".into());
    }
    if tokens.len() >= 120
        && matches!(
            intent.as_str(),
            "brainstorming" | "explaining_concept" | "asking_question"
        )
    {
        patterns.insert("long_exploratory_dictation".into());
    }
    if tokens.len() <= 28
        && matches!(
            intent.as_str(),
            "implementation_instruction" | "drafting_text" | "planning"
        )
    {
        patterns.insert("short_command_dictation".into());
    }
    if contains_any(
        &lower,
        &[
            "primeiro", "segundo", "terceiro", "lista", "etapa 1", "passo 1",
        ],
    ) {
        patterns.insert("structured_enumeration".into());
    }

    SemanticContribution {
        topics,
        intent,
        intent_confidence,
        patterns,
        connector_counts: connector_counts(tokens),
        opener: detect_opener(tokens),
        question_count,
        example_count,
        requirement_addition_count,
        sentence_count,
    }
}

fn topic_context_boost(key: &str, category: &str, app: Option<&str>, domain: Option<&str>) -> f64 {
    let context = format!(
        "{} {} {}",
        category,
        app.unwrap_or_default(),
        domain.unwrap_or_default()
    )
    .to_lowercase();
    match key {
        "voice_ai"
            if context.contains("ai prompt")
                || context.contains("chatgpt")
                || context.contains("gemini") =>
        {
            1.5
        }
        "software"
            if context.contains("programming")
                || context.contains("code")
                || context.contains("github")
                || context.contains("codex") =>
        {
            1.5
        }
        "study" if context.contains("study") => 2.0,
        "writing"
            if context.contains("email")
                || context.contains("document")
                || context.contains("message") =>
        {
            1.5
        }
        _ => 0.0,
    }
}

fn detect_intent(lower: &str, tokens: &[String], category: &str) -> (String, f64) {
    let rules: &[(&str, &[&str])] = &[
        (
            "debugging",
            &[
                "erro",
                "falha",
                "bug",
                "corrija",
                "corrigir",
                "não funciona",
                "stack trace",
                "exception",
            ],
        ),
        (
            "implementation_instruction",
            &[
                "implemente",
                "crie",
                "adicione",
                "altere",
                "faça",
                "preciso que",
                "quero que",
                "deve",
                "não deve",
            ],
        ),
        (
            "brainstorming",
            &[
                "ideia",
                "poderia",
                "talvez",
                "vamos pensar",
                "opções",
                "alternativas",
                "e se",
            ],
        ),
        (
            "explaining_concept",
            &[
                "explica",
                "explique",
                "significa",
                "funciona",
                "conceito",
                "porque",
                "por exemplo",
            ],
        ),
        (
            "studying",
            &[
                "estudar",
                "estudo",
                "prova",
                "exercício",
                "questão",
                "resumo",
                "aula",
            ],
        ),
        (
            "drafting_text",
            &[
                "escreva", "redija", "mensagem", "email", "e-mail", "texto", "resposta",
            ],
        ),
        (
            "planning",
            &[
                "planeje",
                "plano",
                "etapas",
                "cronograma",
                "organize",
                "prioridade",
            ],
        ),
        (
            "asking_question",
            &["como", "por que", "o que", "qual", "quais", "será que"],
        ),
    ];
    let mut scores: Vec<(&str, u64)> = rules
        .iter()
        .map(|(key, markers)| (*key, count_markers(lower, markers)))
        .collect();
    if lower.contains('?') {
        if let Some((_, score)) = scores.iter_mut().find(|(key, _)| *key == "asking_question") {
            *score += 2;
        }
    }
    if category.eq_ignore_ascii_case("study") {
        if let Some((_, score)) = scores.iter_mut().find(|(key, _)| *key == "studying") {
            *score += 2;
        }
    }
    scores.sort_by_key(|item| std::cmp::Reverse(item.1));
    let Some((key, score)) = scores.first().copied().filter(|(_, score)| *score > 0) else {
        return ("unknown".into(), 0.0);
    };
    let confidence =
        (0.45 + score as f64 * 0.12 + (tokens.len() >= 12) as u8 as f64 * 0.08).min(0.95);
    (key.into(), confidence)
}

fn connector_counts(tokens: &[String]) -> BTreeMap<String, u64> {
    let connectors: &[&[&str]] = &[
        &["além", "disso"],
        &["alem", "disso"],
        &["por", "exemplo"],
        &["ou", "seja"],
        &["na", "verdade"],
        &["porém"],
        &["portanto"],
        &["então"],
        &["também"],
        &["mas"],
        &["porque"],
        &["however"],
        &["therefore"],
        &["also"],
        &["because"],
    ];
    let mut counts = BTreeMap::new();
    for connector in connectors {
        let count = tokens
            .windows(connector.len())
            .filter(|window| {
                window
                    .iter()
                    .map(String::as_str)
                    .eq(connector.iter().copied())
            })
            .count() as u64;
        if count > 0 {
            counts.insert(connector.join(" "), count);
        }
    }
    counts
}

fn detect_opener(tokens: &[String]) -> Option<String> {
    let openers: &[&[&str]] = &[
        &["eu", "queria"],
        &["eu", "quero"],
        &["quero", "que"],
        &["gostaria", "de"],
        &["preciso", "que"],
        &["vamos"],
        &["me", "explica"],
        &["me", "explique"],
        &["faça"],
        &["como"],
        &["por", "que"],
        &["i", "want"],
        &["can", "you"],
        &["let's"],
    ];
    openers
        .iter()
        .find(|candidate| {
            candidate.len() <= tokens.len()
                && tokens
                    .iter()
                    .zip(candidate.iter())
                    .all(|(token, expected)| token == expected)
        })
        .map(|candidate| candidate.join(" "))
}

fn count_markers(text: &str, markers: &[&str]) -> u64 {
    markers
        .iter()
        .map(|marker| count_bounded_markers(text, marker))
        .sum()
}

fn count_bounded_markers(text: &str, marker: &str) -> u64 {
    let marker = marker.trim();
    if marker.is_empty() {
        return 0;
    }
    text.match_indices(marker)
        .filter(|(index, matched)| {
            let before_is_word = text[..*index]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric);
            let after_index = *index + matched.len();
            let after_is_word = text[after_index..]
                .chars()
                .next()
                .is_some_and(char::is_alphanumeric);
            !before_is_word && !after_is_word
        })
        .count() as u64
}

fn contains_any(text: &str, markers: &[&str]) -> bool {
    markers
        .iter()
        .any(|marker| count_bounded_markers(text, marker) > 0)
}

pub fn topic_title(key: &str) -> String {
    TOPICS
        .iter()
        .find(|topic| topic.key == key)
        .map(|topic| topic.title.to_string())
        .unwrap_or_else(|| key.replace('_', " "))
}

pub fn intent_title(key: &str) -> String {
    match key {
        "asking_question" => "Fazer perguntas",
        "implementation_instruction" => "Dar instruções de implementação",
        "brainstorming" => "Explorar ideias",
        "explaining_concept" => "Explicar conceitos",
        "studying" => "Estudar",
        "drafting_text" => "Redigir textos",
        "debugging" => "Depurar problemas",
        "planning" => "Planejar",
        _ => "Intenção desconhecida",
    }
    .into()
}

pub fn pattern_title(key: &str) -> String {
    match key {
        "iterative_refinement" => "Refinamento iterativo",
        "uses_examples" => "Uso de exemplos",
        "adds_requirements" => "Requisitos acrescentados em camadas",
        "asks_questions" => "Perguntas frequentes",
        "long_exploratory_dictation" => "Ditados exploratórios longos",
        "short_command_dictation" => "Comandos curtos e diretos",
        "structured_enumeration" => "Estrutura por etapas",
        _ => key,
    }
    .into()
}

pub fn evidence_level(words: u64) -> (&'static str, u64) {
    if words < BASIC_WORDS {
        ("collecting", BASIC_WORDS)
    } else if words < ARCHETYPE_WORDS {
        ("basic", ARCHETYPE_WORDS)
    } else if words < RICH_WORDS {
        ("archetype", RICH_WORDS)
    } else if words < HIGH_CONFIDENCE_WORDS {
        ("rich", HIGH_CONFIDENCE_WORDS)
    } else {
        ("high_confidence", HIGH_CONFIDENCE_WORDS)
    }
}

pub fn overall_confidence(words: u64, sessions: u64, semantic_sessions: u64) -> f64 {
    let word_score = (words as f64 / HIGH_CONFIDENCE_WORDS as f64)
        .sqrt()
        .min(1.0);
    let session_score = (sessions as f64 / 50.0).sqrt().min(1.0);
    let coverage = if sessions == 0 {
        0.0
    } else {
        semantic_sessions as f64 / sessions as f64
    };
    (word_score * 0.45 + session_score * 0.35 + coverage * 0.20).clamp(0.0, 1.0)
}

pub fn parse_and_validate_profile(
    text: &str,
    evidence: &VoiceProfileEvidence,
) -> Result<GeneratedVoiceProfile, ProfileValidationFailure> {
    let cleaned = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let profile: GeneratedVoiceProfile =
        serde_json::from_str(cleaned).map_err(|_| ProfileValidationFailure::InvalidJson)?;
    validate_profile(&profile, evidence)?;
    Ok(profile)
}

fn validate_profile(
    profile: &GeneratedVoiceProfile,
    evidence: &VoiceProfileEvidence,
) -> Result<(), ProfileValidationFailure> {
    let archetype = &profile.archetype;
    let title_words = archetype.title.split_whitespace().count();
    if !(2..=5).contains(&title_words) || archetype.title.chars().count() > 60 {
        return Err(ProfileValidationFailure::SchemaValidation(
            "archetype.title ausente ou longo demais".into(),
        ));
    }
    if archetype.subtitle.trim().is_empty()
        || archetype.description.trim().is_empty()
        || archetype.description.chars().count() > 700
    {
        return Err(ProfileValidationFailure::SchemaValidation(
            "archetype incompleto".into(),
        ));
    }
    if archetype_is_generic(&archetype.title)
        || contains_prohibited_inference(&archetype.title)
        || contains_prohibited_inference(&archetype.subtitle)
        || contains_prohibited_inference(&archetype.description)
    {
        return Err(ProfileValidationFailure::SchemaValidation(
            "archetype genérico ou contém inferência proibida".into(),
        ));
    }
    let valid_keys = evidence_keys(evidence);
    validate_interpretation(
        archetype.confidence,
        &archetype.evidence_keys,
        &valid_keys,
        evidence.coverage.overall_confidence,
        "archetype",
    )?;
    if profile.communication_patterns.len() > 6
        || profile.interesting_observations.len() > 6
        || profile.recurring_topics.len() > 6
        || profile.personal_portrait.distinctive_habits.len() > 4
        || profile.personal_portrait.usage_rhythms.len() > 4
        || profile.suggested_experiments.len() > 4
    {
        return Err(ProfileValidationFailure::SchemaValidation(
            "coleções excedem o limite".into(),
        ));
    }
    let portrait = &profile.personal_portrait;
    if portrait.summary.trim().is_empty()
        || portrait.summary.chars().count() > 1_000
        || contains_prohibited_inference(&portrait.summary)
    {
        return Err(ProfileValidationFailure::SchemaValidation(
            "personal_portrait inválido ou contém inferência proibida".into(),
        ));
    }
    validate_interpretation(
        portrait.confidence,
        &portrait.evidence_keys,
        &valid_keys,
        evidence.coverage.overall_confidence,
        "personal portrait",
    )?;
    for (label, items) in [
        ("distinctive habit", portrait.distinctive_habits.as_slice()),
        ("usage rhythm", portrait.usage_rhythms.as_slice()),
        (
            "suggested experiment",
            profile.suggested_experiments.as_slice(),
        ),
    ] {
        for item in items {
            if item.title.trim().is_empty()
                || item.description.trim().is_empty()
                || contains_prohibited_inference(&item.title)
                || contains_prohibited_inference(&item.description)
            {
                return Err(ProfileValidationFailure::SchemaValidation(format!(
                    "{label} incompleto ou contém inferência proibida"
                )));
            }
            validate_interpretation(
                item.confidence,
                &item.evidence_keys,
                &valid_keys,
                evidence.coverage.overall_confidence,
                label,
            )?;
        }
    }
    if valid_keys.len() >= 2 && portrait.distinctive_habits.is_empty() {
        return Err(ProfileValidationFailure::SchemaValidation(
            "personal_portrait omitiu hábitos distintivos sustentados".into(),
        ));
    }
    if evidence.coverage.words >= RICH_WORDS {
        let required_topics = evidence.recurring_topics.len().min(3);
        if profile.recurring_topics.len() < required_topics {
            return Err(ProfileValidationFailure::SchemaValidation(
                "perfil rico omitiu tópicos recorrentes sustentados".into(),
            ));
        }
        if evidence.linguistic_patterns.len() >= 2 && profile.communication_patterns.is_empty() {
            return Err(ProfileValidationFailure::SchemaValidation(
                "perfil rico omitiu padrões de comunicação sustentados".into(),
            ));
        }
        if evidence_keys(evidence).len() >= 3 && profile.interesting_observations.len() < 3 {
            return Err(ProfileValidationFailure::SchemaValidation(
                "perfil rico requer ao menos três observações sustentadas".into(),
            ));
        }
    }
    for pattern in &profile.communication_patterns {
        if pattern.title.trim().is_empty() || pattern.description.trim().is_empty() {
            return Err(ProfileValidationFailure::SchemaValidation(
                "communication pattern incompleto".into(),
            ));
        }
        if contains_prohibited_inference(&pattern.title)
            || contains_prohibited_inference(&pattern.description)
        {
            return Err(ProfileValidationFailure::SchemaValidation(
                "communication pattern contém inferência proibida".into(),
            ));
        }
        validate_interpretation(
            pattern.confidence,
            &pattern.evidence_keys,
            &valid_keys,
            evidence.coverage.overall_confidence,
            "communication pattern",
        )?;
    }
    for observation in &profile.interesting_observations {
        if observation.title.trim().is_empty() || observation.description.trim().is_empty() {
            return Err(ProfileValidationFailure::SchemaValidation(
                "observation incompleta".into(),
            ));
        }
        if contains_prohibited_inference(&observation.title)
            || contains_prohibited_inference(&observation.description)
        {
            return Err(ProfileValidationFailure::SchemaValidation(
                "observation contém inferência proibida".into(),
            ));
        }
        validate_interpretation(
            observation.confidence,
            &observation.evidence_keys,
            &valid_keys,
            evidence.coverage.overall_confidence,
            "observation",
        )?;
    }
    for topic in &profile.recurring_topics {
        let Some(source) = evidence
            .recurring_topics
            .iter()
            .find(|source| source.title == topic.title)
        else {
            return Err(ProfileValidationFailure::SchemaValidation(
                "tópico não existe nas evidências".into(),
            ));
        };
        if !(0.0..=1.0).contains(&topic.share) || (topic.share - source.share).abs() > 0.025 {
            return Err(ProfileValidationFailure::SchemaValidation(
                "share de tópico diverge das evidências".into(),
            ));
        }
    }
    validate_signature(&profile.signature, &evidence.signature_candidates)?;
    if evidence.coverage.words >= RICH_WORDS {
        let available = signature_value_count(&evidence.signature_candidates);
        let selected = signature_profile_value_count(&profile.signature);
        if available >= 2 && selected < 2 {
            return Err(ProfileValidationFailure::SchemaValidation(
                "perfil rico omitiu candidatos de assinatura disponíveis".into(),
            ));
        }
    }
    Ok(())
}

fn signature_value_count(signature: &SignatureEvidence) -> usize {
    [
        signature.catchphrase.as_ref(),
        signature.content_word.as_ref(),
        signature.phrase.as_ref(),
        signature.connector.as_ref(),
        signature.opener.as_ref(),
    ]
    .into_iter()
    .flatten()
    .count()
}

fn signature_profile_value_count(signature: &SignatureProfile) -> usize {
    [
        signature.catchphrase.as_ref(),
        signature.content_word.as_ref(),
        signature.phrase.as_ref(),
        signature.connector.as_ref(),
        signature.opener.as_ref(),
    ]
    .into_iter()
    .flatten()
    .count()
}

fn archetype_is_generic(title: &str) -> bool {
    matches!(
        title.trim().to_ascii_lowercase().as_str(),
        "voice user"
            | "active dictator"
            | "productive speaker"
            | "usuário de voz"
            | "usuario de voz"
            | "ditador ativo"
    )
}

fn contains_prohibited_inference(value: &str) -> bool {
    let lower = value.to_lowercase();
    [
        "personalidade",
        "personality",
        "inteligência",
        "inteligencia",
        "intelligence",
        "ansiedade",
        "anxiety",
        "depressão",
        "depressao",
        "depression",
        "saúde mental",
        "saude mental",
        "mental health",
        "transtorno",
        "disorder",
        "honestidade",
        "honesty",
        "emoção",
        "emocao",
        "emotion",
        "introvertido",
        "extrovertido",
        "introvert",
        "extrovert",
        "identidade",
        "identity",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn validate_interpretation(
    confidence: f64,
    keys: &[String],
    valid_keys: &BTreeSet<String>,
    overall_confidence: f64,
    label: &str,
) -> Result<(), ProfileValidationFailure> {
    if !(0.0..=1.0).contains(&confidence)
        || confidence > (overall_confidence + 0.15).min(1.0)
        || keys.is_empty()
        || keys.iter().any(|key| !valid_keys.contains(key))
    {
        return Err(ProfileValidationFailure::SchemaValidation(format!(
            "{label} possui confidence/evidence_keys inválidos"
        )));
    }
    Ok(())
}

fn validate_signature(
    signature: &SignatureProfile,
    candidates: &SignatureEvidence,
) -> Result<(), ProfileValidationFailure> {
    for (value, expected, label) in [
        (
            &signature.catchphrase,
            &candidates.catchphrase,
            "catchphrase",
        ),
        (
            &signature.content_word,
            &candidates.content_word,
            "content_word",
        ),
        (&signature.phrase, &candidates.phrase, "phrase"),
        (&signature.connector, &candidates.connector, "connector"),
        (&signature.opener, &candidates.opener, "opener"),
    ] {
        if value.is_some() && value != expected {
            return Err(ProfileValidationFailure::SchemaValidation(format!(
                "signature.{label} não pertence aos candidatos"
            )));
        }
    }
    Ok(())
}

pub fn evidence_keys(evidence: &VoiceProfileEvidence) -> BTreeSet<String> {
    evidence
        .recurring_topics
        .iter()
        .chain(&evidence.recurring_intents)
        .chain(&evidence.linguistic_patterns)
        .chain(&evidence.correction_patterns)
        .chain(&evidence.application_patterns)
        .chain(&evidence.workflow_patterns)
        .chain(&evidence.acoustic_patterns)
        .chain(&evidence.temporal_patterns)
        .chain(&evidence.trends)
        .map(|item| item.key.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(text: &str) -> Vec<String> {
        text.to_lowercase()
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn semantic_topics_group_related_concepts() {
        let biology = analyze_semantics(
            "DNA cromossomos mitose célula",
            &tokens("DNA cromossomos mitose célula"),
            "Study",
            None,
            None,
            0,
        );
        assert!(biology.topics.contains_key("biology"));
        let voice = analyze_semantics(
            "OpenRouter Gemini Deepgram pipeline provider",
            &tokens("OpenRouter Gemini Deepgram pipeline provider"),
            "Programming",
            Some("Codex"),
            None,
            0,
        );
        assert!(voice.topics.contains_key("voice_ai"));
    }

    #[test]
    fn intent_and_patterns_are_calculable() {
        let result = analyze_semantics(
            "Implemente esta função. Além disso, adicione testes e também valide o erro.",
            &tokens("Implemente esta função além disso adicione testes e também valide o erro"),
            "Programming",
            Some("Codex"),
            None,
            0,
        );
        assert_eq!(result.intent, "implementation_instruction");
        assert!(result.patterns.contains("adds_requirements"));
    }

    #[test]
    fn signature_candidates_extract_connector_and_opener() {
        let result = analyze_semantics(
            "Eu queria melhorar isso. Além disso, faça testes.",
            &tokens("Eu queria melhorar isso além disso faça testes"),
            "Unknown",
            None,
            None,
            0,
        );
        assert_eq!(result.opener.as_deref(), Some("eu queria"));
        assert_eq!(result.connector_counts["além disso"], 1);
    }

    #[test]
    fn semantic_markers_respect_word_boundaries() {
        let result = analyze_semantics(
            "A acomodação ficou capitalizada.",
            &tokens("A acomodação ficou capitalizada"),
            "Unknown",
            None,
            None,
            0,
        );
        assert_eq!(result.intent, "unknown");
        assert!(!result.topics.contains_key("software"));
    }

    #[test]
    fn progressive_levels_unlock_at_expected_thresholds() {
        assert_eq!(evidence_level(499), ("collecting", 500));
        assert_eq!(evidence_level(500), ("basic", 2_000));
        assert_eq!(evidence_level(2_000), ("archetype", 5_000));
        assert_eq!(evidence_level(5_000), ("rich", 10_000));
        assert_eq!(evidence_level(10_000), ("high_confidence", 10_000));
    }

    #[test]
    fn confidence_increases_with_sample_and_coverage() {
        assert!(overall_confidence(10_000, 50, 50) > overall_confidence(500, 3, 1));
    }

    #[test]
    fn invalid_json_and_unlinked_evidence_are_rejected() {
        let mut evidence = VoiceProfileEvidence::default();
        evidence.recurring_topics.push(EvidenceItem {
            key: "topic:software:0.50".into(),
            title: "Desenvolvimento de software".into(),
            ..EvidenceItem::default()
        });
        assert!(matches!(
            parse_and_validate_profile("not json", &evidence),
            Err(ProfileValidationFailure::InvalidJson)
        ));
        let invalid = r#"{"archetype":{"title":"Builder","subtitle":"Uso técnico","description":"Descrição","confidence":0.8,"evidence_keys":["invented"]},"signature":{},"communication_patterns":[],"recurring_topics":[],"interesting_observations":[]}"#;
        assert!(matches!(
            parse_and_validate_profile(invalid, &evidence),
            Err(ProfileValidationFailure::SchemaValidation(_))
        ));
    }
}
