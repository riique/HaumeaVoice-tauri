import type { InsightsResponse, VoiceProfile } from "../../src/lib/tauri";

/** Invented demonstration data. No user history or external generation. */
export function insightsFixture(withProfile = true): InsightsResponse {
  const data: InsightsResponse = {
    analysis_version: 3, period: "last30_days", generated_at_ms: 1,
    usage: { sessions: 48, words: 1240, audio_duration_ms: 620000, average_wpm: 120, manual_corrections: 2, vocabulary_corrections: 1 },
    language: { language: "pt-BR", fillers: [], catchphrase_ready: true, profile_ready: true, most_corrected: { before: "planejar", after: "planejamento", count: 2, in_vocabulary: false } },
    audio: { analyzed_sessions: 40, coverage_percentage: 83, lufs_median: -31, rms_dbfs_median: -30, peak_dbfs_median: -12, estimated_snr_db: 22 },
    applications: [], application_details: [], domains: [], categories: [], trends: [],
    temporal: { current_streak_days: 3, longest_streak_days: 6, activity: [] },
    voice_evidence: {
      statistics: { sessions: 48, words: 1240, manual_corrections: 2 },
      recurring_topics: [{ key: "topic", title: "Projetos e ideias", description: "", count: 16, share: .4, confidence: .8 }],
      recurring_intents: [], linguistic_patterns: [], correction_patterns: [], application_patterns: [], workflow_patterns: [], acoustic_patterns: [], temporal_patterns: [], trends: [],
      signature_candidates: { catchphrase: "Vamos por partes", connector: "por exemplo", content_word: "ideia" },
      coverage: { level: "archetype", overall_confidence: .8, session_coverage: 1, audio_coverage: .83, words: 1240, sessions: 48, next_level_words: 2000 },
    },
    profile_enabled: true, profile_progress_words: 1240, profile_required_words: 2000, profile_generation_ready: false,
    backfill: { running: false, paused: false, processed: 48, total: 48, analyzed: 40, unavailable_audio: 8 },
  };
  if (withProfile) {
    const pattern = (title: string, description: string) => ({ title, description, confidence: .8, evidence_keys: ["synthetic"] });
    const profile: VoiceProfile = {
      title: "Ideias que ganham forma", description: "",
      archetype: { title: "Ideias que ganham forma", subtitle: "", description: "", confidence: .8, evidence_keys: [] },
      personal_portrait: { summary: "Você usa a fala para organizar o que está pensando. Costuma começar com uma ideia e dar forma a ela com exemplos, como quem conversa para chegar a uma conclusão.", confidence: .8, evidence_keys: [], distinctive_habits: [pattern("Pensa em voz alta", "Suas ideias se desenvolvem enquanto você fala, um passo de cada vez.")], usage_rhythms: [] },
      signature: { catchphrase: "Vamos por partes", content_word: "ideia", connector: "por exemplo", opener: "Acho que" },
      communication_patterns: [pattern("Traz exemplos", "Você costuma explicar o que quer dizer com situações do dia a dia."), pattern("Vai direto ao ponto", "Seus ditados geralmente começam pelo que você precisa resolver.")],
      recurring_topics: [{ title: "Projetos e ideias", description: "", share: .4 }, { title: "Planos do dia", description: "", share: .3 }, { title: "Conversas", description: "", share: .3 }],
      interesting_observations: [], suggested_experiments: [pattern("Dê um título à ideia", "Antes de desenvolver um pensamento, diga em uma frase o que você quer guardar.")],
      generated_at_ms: 1, generated_at_word_count: 500, next_update_word_count: 2000, profile_version: 3,
      provider: "synthetic", model: "synthetic", request_ms: 0, bytes_sent: 0, evidence_bundle: data.voice_evidence,
      sanitized_prompt: "Synthetic fixture", sanitized_response: "Synthetic fixture", schema_validation: "valid",
    };
    data.profile = profile;
  }
  return data;
}
