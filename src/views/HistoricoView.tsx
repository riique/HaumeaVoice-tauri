import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Search,
  Trash2,
  Sparkles,
  Loader2,
  ChevronDown,
  AlertCircle,
  RefreshCw,
  Braces,
  Copy,
  Pencil,
  Check,
  Info,
  FolderOpen,
} from "lucide-react";
import { Button } from "../components/ui/Button";
import { Card } from "../components/ui/Card";
import { Input } from "../components/ui/Input";
import { PronunciationEvaluation } from "../components/PronunciationEvaluation";
import {
  evaluatePronunciation,
  getDevMode,
  revealHistoryAudio,
  type SanitizerDebug,
} from "../lib/tauri";

interface HistoryEntry {
  id: string;
  date: string;
  words: number;
  engine: string;
  text: string;
  audio_path?: string | null;
  evaluation?: string | null;
  duration_ms?: number;
  source?: string;
  latency_ms?: number;
  throughput?: number;
  transcription_latency_ms?: number | null;
  sanitizer_latency_ms?: number | null;
  transcription_throughput?: number | null;
  sanitizer_throughput?: number | null;
  realtime_factor?: number | null;
  deepgram_mode?: string | null;
  total_tokens?: number | null;
  is_error?: boolean;
  error_message?: string | null;
  debug_info?: SanitizerDebug | null;
  mode?: string | null;
  model?: string | null;
  stages?: string | null;
  used_fallback?: boolean | null;
  fallback_reason?: string | null;
  content_type?: string | null;
  whisper_text?: string | null;
  sanitizer_text?: string | null;
  gemini_text?: string | null;
  warnings?: string[] | null;
  audio_prepare_ms?: number | null;
  base64_ms?: number | null;
  whisper_ms?: number | null;
  sanitizer_ms?: number | null;
  files_upload_ms?: number | null;
  files_poll_ms?: number | null;
  files_poll_count?: number | null;
  gemini_generate_ms?: number | null;
  gemini_delete_ms?: number | null;
  strict_literals_ms?: number | null;
  clipboard_ms?: number | null;
  total_pipeline_ms?: number | null;
  gemini_transport?: string | null;
}

function fmtSec(ms: number | null | undefined): string | null {
  if (ms === undefined || ms === null) return null;
  return `${(ms / 1000).toFixed(2)} s`;
}

function transportLabel(t: string | null | undefined): string | null {
  if (!t) return null;
  if (t === "inline") return "Gemini inline";
  if (t === "files_api") return "Gemini Files API";
  return t;
}

function hasStructuredTiming(h: HistoryEntry): boolean {
  return Boolean(
    h.total_pipeline_ms != null ||
      h.whisper_ms != null ||
      h.gemini_generate_ms != null ||
      h.sanitizer_ms != null ||
      h.base64_ms != null ||
      h.files_upload_ms != null ||
      h.gemini_transport,
  );
}

function latencySummary(h: HistoryEntry): string | null {
  const total =
    h.total_pipeline_ms ??
    h.transcription_latency_ms ??
    (h.latency_ms && h.latency_ms > 0 ? h.latency_ms : null);
  if (total == null) return null;
  if (!hasStructuredTiming(h) && h.mode == null) {
    // Legacy two-stage display only when no product mode metadata.
    if (
      h.transcription_latency_ms != null &&
      h.sanitizer_latency_ms != null &&
      h.mode == null &&
      h.gemini_transport == null
    ) {
      return `${(h.transcription_latency_ms / 1000).toFixed(2)}s motor + ${(h.sanitizer_latency_ms / 1000).toFixed(2)}s sanitizador`;
    }
    return `${(total / 1000).toFixed(2)}s latência`;
  }
  const parts: string[] = [`Total ${(total / 1000).toFixed(2)} s`];
  const tr = transportLabel(h.gemini_transport);
  if (tr) parts.push(tr);
  const bits: string[] = [];
  if (h.whisper_ms != null) bits.push(`Whisper ${(h.whisper_ms / 1000).toFixed(2)} s`);
  if (h.sanitizer_ms != null) bits.push(`Sanitizador ${(h.sanitizer_ms / 1000).toFixed(2)} s`);
  if (h.gemini_generate_ms != null)
    bits.push(`Gemini ${(h.gemini_generate_ms / 1000).toFixed(2)} s`);
  if (bits.length) parts.push(bits.join(" · "));
  return parts.join(" · ");
}

function deepgramModeLabel(mode: string | null | undefined): string | null {
  if (!mode) return null;
  if (mode === "streaming_final") return "DG streaming";
  if (mode === "batch") return "DG batch";
  return mode;
}

function productModeLabel(mode: string | null | undefined): string | null {
  if (!mode) return null;
  if (mode === "ultra-fast") return "Ultrarrápido";
  if (mode === "fast-accurate") return "Rápido e preciso";
  if (mode === "precise") return "Preciso";
  if (mode === "ultra-precise") return "Ultrapreciso";
  return mode;
}

function formatHistoryForClipboard(items: HistoryEntry[]): string {
  return items
    .filter((item) => !item.is_error && item.text.trim())
    .map((item, index) => {
      const model = item.model?.trim() || item.engine?.trim() || "Não informado";
      const pipeline = productModeLabel(item.mode) || "Legado";
      const stages = item.stages
        ?.split(",")
        .map((stage) => stage.trim())
        .filter(Boolean)
        .join(" → ");

      return [
        `=== Transcrição ${index + 1} ===`,
        `Data: ${item.date}`,
        `Modelo: ${model}`,
        `Pipeline: ${pipeline}`,
        ...(stages ? [`Etapas: ${stages}`] : []),
        "",
        item.text.trim(),
      ].join("\n");
    })
    .join("\n\n");
}

/** Synthetic debug when entry has mode metadata but no persisted request (pre-fix). */
function pipelineDebugFromEntry(h: HistoryEntry): SanitizerDebug {
  const parts: string[] = [];
  if (h.whisper_text) parts.push(`[WHISPER]\n${h.whisper_text}`);
  if (h.sanitizer_text) parts.push(`[SANITIZER]\n${h.sanitizer_text}`);
  if (h.gemini_text) parts.push(`[GEMINI]\n${h.gemini_text}`);
  if (parts.length === 0 && h.text) parts.push(`[FINAL]\n${h.text}`);
  const stages = h.stages
    ? h.stages.split(",").map((s) => s.trim()).filter(Boolean)
    : [];
  return {
    endpoint: `product-mode:${h.mode || "unknown"}`,
    model: h.model || "—",
    temperature: 0,
    reasoning_enabled: false,
    reasoning_effort: "",
    reasoning_effort_applied: false,
    reasoning_supported_by_model: false,
    system_prompt: stages.length
      ? `Pipeline de produto.\nEtapas: ${stages.join(" → ")}`
      : "Pipeline de produto (sem etapas gravadas).",
    user_message: parts.join("\n\n") || "(sem intermediários)",
    request_json: JSON.stringify(
      {
        kind: "product_pipeline",
        mode: h.mode,
        model: h.model,
        stages,
        used_fallback: h.used_fallback,
        fallback_reason: h.fallback_reason,
        engine: h.engine,
      },
      null,
      2,
    ),
    response_status: 200,
    response_content: h.text || null,
    response_reasoning: null,
    error: h.fallback_reason || null,
  };
}

function resolveDebugInfo(h: HistoryEntry): SanitizerDebug | null {
  if (h.debug_info) return h.debug_info;
  if (h.mode || h.stages || h.model) return pipelineDebugFromEntry(h);
  return null;
}

export function HistoricoView() {
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [evaluatingId, setEvaluatingId] = useState<string | null>(null);
  const [retryingId, setRetryingId] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [devMode, setDevMode] = useState(false);
  const [devExpanded, setDevExpanded] = useState<Record<string, boolean>>({});
  const [detailsOpen, setDetailsOpen] = useState<Record<string, boolean>>({});
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editDraft, setEditDraft] = useState("");
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [copiedAll, setCopiedAll] = useState(false);
  const [copyAllError, setCopyAllError] = useState("");

  const refresh = async () => {
    try {
      const data = await invoke<HistoryEntry[]>("get_history");
      setItems(data);
    } catch (e) {
      console.error("failed to load history:", e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();

    getDevMode()
      .then(setDevMode)
      .catch((e) => console.error("getDevMode failed:", e));

    // Live-update when a new transcription is saved while this view is open.
    const unlistenPromise = listen("transcription-saved", () => refresh());
    return () => {
      unlistenPromise.then((u) => u());
    };
  }, []);

  const handleClearAll = async () => {
    try {
      await invoke("clear_history");
      setItems([]);
    } catch (e) {
      console.error("failed to clear history:", e);
    }
  };

  const handleEvaluate = async (id: string) => {
    // If feedback already exists, the button just toggles the panel.
    const existing = items.find((h) => h.id === id);
    if (existing?.evaluation) {
      setExpanded((s) => ({ ...s, [id]: !s[id] }));
      return;
    }

    setEvaluatingId(id);
    setErrors((e) => ({ ...e, [id]: "" }));
    try {
      const feedback = await evaluatePronunciation(id);
      setItems((prev) =>
        prev.map((h) => (h.id === id ? { ...h, evaluation: feedback } : h)),
      );
      setExpanded((s) => ({ ...s, [id]: true }));
    } catch (e) {
      setErrors((errs) => ({
        ...errs,
        [id]: typeof e === "string" ? e : String(e),
      }));
    } finally {
      setEvaluatingId(null);
    }
  };

  const handleRetry = async (id: string) => {
    setRetryingId(id);
    setErrors((e) => ({ ...e, [id]: "" }));
    try {
      await invoke("retry_transcription", { id });
      await refresh();
    } catch (e) {
      console.error("failed to retry transcription:", e);
      setErrors((errs) => ({
        ...errs,
        [id]: typeof e === "string" ? e : String(e),
      }));
    } finally {
      setRetryingId(null);
    }
  };

  const handleCopy = async (id: string, text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedId(id);
      window.setTimeout(() => setCopiedId((c) => (c === id ? null : c)), 1500);
    } catch (e) {
      setErrors((errs) => ({
        ...errs,
        [id]: "Não foi possível copiar.",
      }));
    }
  };

  const handleRevealAudio = async (id: string) => {
    setErrors((current) => ({ ...current, [id]: "" }));
    try {
      await revealHistoryAudio(id);
    } catch (error) {
      setErrors((current) => ({
        ...current,
        [id]: typeof error === "string" ? error : String(error),
      }));
    }
  };

  const handleCopyAll = async () => {
    const text = formatHistoryForClipboard(items);
    if (!text) return;

    try {
      await navigator.clipboard.writeText(text);
      setCopyAllError("");
      setCopiedAll(true);
      window.setTimeout(() => setCopiedAll(false), 1500);
    } catch (e) {
      console.error("failed to copy all transcriptions:", e);
      setCopiedAll(false);
      setCopyAllError("Não foi possível copiar todas as transcrições.");
    }
  };

  const handleDelete = async (id: string) => {
    if (!window.confirm("Excluir esta transcrição? O áudio salvo também será removido.")) {
      return;
    }
    try {
      await invoke("delete_history_entry", { id });
      setItems((prev) => prev.filter((h) => h.id !== id));
    } catch (e) {
      setErrors((errs) => ({
        ...errs,
        [id]: typeof e === "string" ? e : String(e),
      }));
    }
  };

  const handleSaveEdit = async (id: string) => {
    try {
      await invoke("update_history_text", { id, text: editDraft });
      setItems((prev) =>
        prev.map((h) =>
          h.id === id
            ? {
                ...h,
                text: editDraft,
                words: editDraft.trim() ? editDraft.trim().split(/\s+/).length : 0,
                is_error: false,
                error_message: null,
              }
            : h,
        ),
      );
      setEditingId(null);
    } catch (e) {
      setErrors((errs) => ({
        ...errs,
        [id]: typeof e === "string" ? e : String(e),
      }));
    }
  };

  const filtered = query.trim()
    ? items.filter((h) =>
        h.text.toLowerCase().includes(query.trim().toLowerCase()) ||
        (h.error_message && h.error_message.toLowerCase().includes(query.trim().toLowerCase()))
      )
    : items;
  const copyableCount = items.filter((item) => !item.is_error && item.text.trim()).length;

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight text-zinc-100">
          Histórico
        </h1>
        <p className="mt-1 text-sm text-zinc-500">
          Suas transcrições salvas, ordenadas por data.
        </p>
      </header>

      {/* Barra de pesquisa */}
      <div className="flex flex-wrap items-center gap-3">
        <div className="relative flex-1">
          <Search className="absolute left-4 top-1/2 h-4 w-4 -translate-y-1/2 text-zinc-500" />
          <Input
            placeholder="Pesquisar transcrições..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            className="pl-11"
          />
        </div>
        <Button
          variant="secondary"
          className="gap-2"
          disabled={copyableCount === 0}
          onClick={handleCopyAll}
          aria-label={`Copiar todas as ${copyableCount} transcrições`}
        >
          {copiedAll ? (
            <Check className="h-4 w-4 text-emerald-400" />
          ) : (
            <Copy className="h-4 w-4" />
          )}
          {copiedAll ? "Todas copiadas" : "Copiar todas"}
        </Button>
        <Button
          variant="danger"
          className="gap-2"
          disabled={items.length === 0}
          onClick={handleClearAll}
        >
          <Trash2 className="h-4 w-4" />
          Limpar Tudo
        </Button>
        {copyAllError ? (
          <p className="basis-full text-right text-xs text-red-400" role="alert">
            {copyAllError}
          </p>
        ) : null}
      </div>

      {/* Lista de cards / estado vazio */}
      {loading ? null : filtered.length === 0 ? (
        <Card className="p-12 text-center">
          <p className="text-sm text-zinc-500">
            {query.trim()
              ? "Nenhuma transcrição encontrada para a busca."
              : "Nenhuma transcrição salva ainda. Grave algo para vê-la aqui."}
          </p>
        </Card>
      ) : (
        <div className="space-y-4">
          {filtered.map((h) => {
            const isEvaluating = evaluatingId === h.id;
            const isRetrying = retryingId === h.id;
            const isError = Boolean(h.is_error);
            const isOpen = expanded[h.id];
            const hasAudio = Boolean(h.audio_path);
            return (
              <Card key={h.id} className="p-7">
                <div className="mb-3 flex items-start justify-between gap-4">
                  <div className="flex items-center gap-3 flex-wrap">
                    <span className="text-xs font-medium text-zinc-500">
                      {h.date}
                    </span>
                    {isError ? (
                      <>
                        <span className="text-xs text-zinc-700">•</span>
                        <span className="inline-flex items-center gap-1 rounded bg-red-500/10 px-2 py-0.5 text-[10px] font-medium text-red-400 border border-red-500/20">
                          Falha
                        </span>
                        <span className="text-xs text-zinc-700">•</span>
                        <span className="text-xs text-zinc-500">{h.engine}</span>
                      </>
                    ) : (
                      <>
                        <span className="text-xs text-zinc-700">•</span>
                        <span className="text-xs text-zinc-500">
                          {h.words} palavras
                        </span>
                        <span className="text-xs text-zinc-700">•</span>
                        <span className="text-xs text-zinc-500">{h.engine}</span>
                        {h.total_tokens !== undefined && h.total_tokens !== null && (
                          <>
                            <span className="text-xs text-zinc-700">•</span>
                            <span className="text-xs text-coral-400">
                              {h.total_tokens} tokens
                            </span>
                          </>
                        )}
                        {productModeLabel(h.mode) && (
                          <>
                            <span className="text-xs text-zinc-700">•</span>
                            <span className="inline-flex items-center rounded bg-coral-500/10 px-2 py-0.5 text-[10px] font-medium text-coral-400 border border-coral-500/20">
                              {productModeLabel(h.mode)}
                            </span>
                          </>
                        )}
                        {h.model && (
                          <>
                            <span className="text-xs text-zinc-700">•</span>
                            <span className="text-xs font-mono text-zinc-500">{h.model}</span>
                          </>
                        )}
                        {h.used_fallback && (
                          <>
                            <span className="text-xs text-zinc-700">•</span>
                            <span
                              className="inline-flex items-center rounded bg-amber-500/10 px-2 py-0.5 text-[10px] font-medium text-amber-400 border border-amber-500/20"
                              title={h.fallback_reason || "Fallback"}
                            >
                              Fallback
                            </span>
                          </>
                        )}
                        {deepgramModeLabel(h.deepgram_mode) && (
                          <>
                            <span className="text-xs text-zinc-700">•</span>
                            <span className="inline-flex items-center rounded bg-coral-500/10 px-2 py-0.5 text-[10px] font-medium text-coral-400 border border-coral-500/20">
                              {deepgramModeLabel(h.deepgram_mode)}
                            </span>
                          </>
                        )}
                        {latencySummary(h) && (
                          <>
                            <span className="text-xs text-zinc-700">•</span>
                            <span className="text-xs text-zinc-500" title="Tempo total da pipeline">
                              {latencySummary(h)}
                            </span>
                          </>
                        )}
                        {h.realtime_factor !== undefined && h.realtime_factor !== null && h.realtime_factor > 0 && (
                          <>
                            <span className="text-xs text-zinc-700">•</span>
                            <span
                              className="text-xs text-zinc-500"
                              title="Fator de tempo real: latência total ÷ duração do áudio. Menor que 1,0 = mais rápido que tempo real."
                            >
                              RTF {h.realtime_factor.toFixed(2)}×
                            </span>
                          </>
                        )}
                      </>
                    )}
                  </div>

                  <div className="flex items-center gap-2 shrink-0 flex-wrap justify-end">
                    {!isError && h.text && (
                      <Button
                        variant="secondary"
                        className="gap-1.5 px-2.5 py-1.5 text-xs"
                        onClick={() => handleCopy(h.id, h.text)}
                        aria-label="Copiar texto"
                      >
                        {copiedId === h.id ? (
                          <Check className="h-3.5 w-3.5 text-emerald-400" />
                        ) : (
                          <Copy className="h-3.5 w-3.5" />
                        )}
                        {copiedId === h.id ? "Copiado" : "Copiar"}
                      </Button>
                    )}
                    {!isError && (
                      <Button
                        variant="secondary"
                        className="gap-1.5 px-2.5 py-1.5 text-xs"
                        onClick={() => {
                          if (editingId === h.id) {
                            setEditingId(null);
                          } else {
                            setEditingId(h.id);
                            setEditDraft(h.text);
                          }
                        }}
                      >
                        <Pencil className="h-3.5 w-3.5" />
                        {editingId === h.id ? "Cancelar" : "Editar"}
                      </Button>
                    )}
                    <Button
                      variant="secondary"
                      className="gap-1.5 px-2.5 py-1.5 text-xs"
                      onClick={() =>
                        setDetailsOpen((s) => ({ ...s, [h.id]: !s[h.id] }))
                      }
                    >
                      <Info className="h-3.5 w-3.5 text-coral-400" />
                      {detailsOpen[h.id] ? "Ocultar detalhes" : "Detalhes"}
                    </Button>
                    <Button
                      variant="secondary"
                      className="gap-1.5 px-2.5 py-1.5 text-xs"
                      disabled={!hasAudio}
                      onClick={() => handleRevealAudio(h.id)}
                      title={
                        hasAudio
                          ? "Mostrar o arquivo de áudio no Explorer"
                          : "Este item antigo não possui áudio salvo"
                      }
                    >
                      <FolderOpen className="h-3.5 w-3.5 text-coral-400" />
                      Mostrar áudio
                    </Button>
                    {devMode && resolveDebugInfo(h) && (
                      <Button
                        variant="secondary"
                        className="gap-2 px-3 py-1.5 text-xs"
                        onClick={() =>
                          setDevExpanded((s) => ({ ...s, [h.id]: !s[h.id] }))
                        }
                      >
                        <Braces className="h-3.5 w-3.5 text-coral-400" />
                        {devExpanded[h.id] ? "Ocultar request" : "Request"}
                      </Button>
                    )}
                    {isError ? (
                      hasAudio && (
                        <Button
                          variant="primary"
                          className="gap-2 px-3 py-1.5 text-xs bg-red-600/90 hover:bg-red-600 text-white font-medium border border-red-500/20"
                          disabled={isRetrying}
                          onClick={() => handleRetry(h.id)}
                        >
                          {isRetrying ? (
                            <Loader2 className="h-3.5 w-3.5 animate-spin" />
                          ) : (
                            <RefreshCw className="h-3.5 w-3.5" />
                          )}
                          {isRetrying ? "Retranscrevendo…" : "Retranscrever"}
                        </Button>
                      )
                    ) : (
                      hasAudio && (
                        <>
                          <Button
                            variant="secondary"
                            className="gap-2 px-3 py-1.5 text-xs"
                            disabled={isRetrying}
                            onClick={() => handleRetry(h.id)}
                            title="Roda de novo com a pipeline atual"
                          >
                            {isRetrying ? (
                              <Loader2 className="h-3.5 w-3.5 animate-spin" />
                            ) : (
                              <RefreshCw className="h-3.5 w-3.5" />
                            )}
                            Retranscrever
                          </Button>
                          <Button
                            variant="secondary"
                            className="gap-2 px-3 py-1.5 text-xs"
                            disabled={isEvaluating}
                            onClick={() => handleEvaluate(h.id)}
                          >
                            {isEvaluating ? (
                              <Loader2 className="h-3.5 w-3.5 animate-spin" />
                            ) : h.evaluation ? (
                              <ChevronDown
                                className={
                                  "h-3.5 w-3.5 transition-transform " +
                                  (isOpen ? "rotate-180" : "")
                                }
                              />
                            ) : (
                              <Sparkles className="h-3.5 w-3.5 text-coral-400" />
                            )}
                            {isEvaluating
                              ? "Avaliando…"
                              : h.evaluation
                                ? isOpen
                                  ? "Ocultar pronúncia"
                                  : "Ver pronúncia"
                                : "Avaliar pronúncia"}
                          </Button>
                        </>
                      )
                    )}
                    <Button
                      variant="danger"
                      className="gap-1.5 px-2.5 py-1.5 text-xs"
                      onClick={() => handleDelete(h.id)}
                      aria-label="Excluir entrada"
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </div>

                {isError ? (
                  <div className="rounded-xl border border-red-900/30 bg-red-950/20 p-4">
                    <div className="flex gap-2">
                      <AlertCircle className="h-5 w-5 text-red-400 shrink-0 mt-0.5" />
                      <div className="space-y-1">
                        <h4 className="text-sm font-semibold text-red-200">
                          Falha na transcrição
                        </h4>
                        <p className="text-sm leading-relaxed text-red-300/80 whitespace-pre-line">
                          {h.error_message ||
                            "Ocorreu um erro desconhecido ao processar o áudio."}
                        </p>
                      </div>
                    </div>
                  </div>
                ) : editingId === h.id ? (
                  <div className="space-y-3">
                    <textarea
                      value={editDraft}
                      onChange={(e) => setEditDraft(e.target.value)}
                      className="w-full min-h-[120px] rounded-xl border border-zinc-800 bg-zinc-950/60 px-4 py-3 text-[15px] leading-relaxed text-zinc-200 outline-none focus:border-coral-500/40 focus-visible:ring-2 focus-visible:ring-coral-500/30"
                      aria-label="Editar texto da transcrição"
                    />
                    <div className="flex gap-2">
                      <Button
                        variant="primary"
                        className="text-xs"
                        onClick={() => handleSaveEdit(h.id)}
                      >
                        Salvar texto
                      </Button>
                      <Button
                        variant="secondary"
                        className="text-xs"
                        onClick={() => setEditingId(null)}
                      >
                        Cancelar
                      </Button>
                    </div>
                  </div>
                ) : (
                  <p className="text-[15px] leading-relaxed text-zinc-300 whitespace-pre-wrap">
                    {h.text}
                  </p>
                )}

                {detailsOpen[h.id] && (
                  <div className="mt-4 rounded-xl border border-zinc-800/60 bg-zinc-950/40 p-4 space-y-4 text-xs text-zinc-500">
                    <div>
                      <p className="text-[10px] font-medium uppercase tracking-wider text-zinc-500">
                        Tempo total e etapas
                      </p>
                      {hasStructuredTiming(h) ? (
                        <div className="mt-2 overflow-hidden rounded-lg border border-zinc-800/80">
                          <table className="w-full text-left">
                            <thead>
                              <tr className="border-b border-zinc-800/80 text-[10px] uppercase tracking-wider text-zinc-600">
                                <th className="px-3 py-2 font-medium">Etapa</th>
                                <th className="px-3 py-2 font-medium text-right">Tempo</th>
                              </tr>
                            </thead>
                            <tbody className="divide-y divide-zinc-800/60">
                              {(
                                [
                                  ["Preparação", h.audio_prepare_ms],
                                  ["Base64", h.base64_ms],
                                  ["Whisper", h.whisper_ms],
                                  ["Sanitizador", h.sanitizer_ms ?? h.sanitizer_latency_ms],
                                  ["Upload", h.files_upload_ms],
                                  [
                                    "Polling",
                                    h.files_poll_ms,
                                    h.files_poll_count != null
                                      ? `${h.files_poll_count}×`
                                      : undefined,
                                  ],
                                  ["Gemini", h.gemini_generate_ms],
                                  ["Delete", h.gemini_delete_ms],
                                  ["Literais", h.strict_literals_ms],
                                  ["Clipboard", h.clipboard_ms],
                                  [
                                    "Total",
                                    h.total_pipeline_ms ??
                                      h.transcription_latency_ms ??
                                      h.latency_ms,
                                  ],
                                ] as [string, number | null | undefined, string?][]
                              )
                                .filter(([, ms]) => ms != null)
                                .map(([label, ms, extra]) => (
                                  <tr key={label}>
                                    <td className="px-3 py-1.5 text-zinc-400">
                                      {label}
                                      {extra ? (
                                        <span className="ml-1 text-zinc-600">({extra})</span>
                                      ) : null}
                                    </td>
                                    <td className="px-3 py-1.5 text-right font-mono text-zinc-300">
                                      {fmtSec(ms)}
                                    </td>
                                  </tr>
                                ))}
                            </tbody>
                          </table>
                        </div>
                      ) : (
                        <p className="mt-2 text-zinc-500 leading-relaxed">
                          Detalhamento indisponível para esta transcrição.
                          {h.transcription_latency_ms != null &&
                          h.sanitizer_latency_ms != null ? (
                            <span className="block mt-1 text-zinc-600">
                              Registro legado:{" "}
                              {(h.transcription_latency_ms / 1000).toFixed(2)}s motor +{" "}
                              {(h.sanitizer_latency_ms / 1000).toFixed(2)}s sanitizador
                            </span>
                          ) : null}
                        </p>
                      )}
                    </div>
                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                      <div>
                        <span className="text-zinc-600">Modo:</span>{" "}
                        {productModeLabel(h.mode) || "legado"}
                      </div>
                      <div>
                        <span className="text-zinc-600">Transporte Gemini:</span>{" "}
                        {transportLabel(h.gemini_transport) || "—"}
                      </div>
                      <div>
                        <span className="text-zinc-600">Modelo:</span>{" "}
                        <span className="font-mono">{h.model || "—"}</span>
                      </div>
                      <div>
                        <span className="text-zinc-600">Fallback utilizado:</span>{" "}
                        {h.used_fallback
                          ? h.fallback_reason || "sim"
                          : "não"}
                      </div>
                      <div>
                        <span className="text-zinc-600">Conteúdo:</span>{" "}
                        {h.content_type || "—"}
                      </div>
                      <div>
                        <span className="text-zinc-600">Estágios:</span>{" "}
                        <span className="font-mono break-all">{h.stages || "—"}</span>
                      </div>
                      <div>
                        <span className="text-zinc-600">Áudio:</span>{" "}
                        {h.audio_path ? "salvo" : "não"}
                      </div>
                      <div>
                        <span className="text-zinc-600">Fonte:</span> {h.source || "—"}
                      </div>
                    </div>
                    {h.warnings && h.warnings.length > 0 && (
                      <p>
                        <span className="text-zinc-600">Avisos:</span>{" "}
                        {h.warnings.join(" · ")}
                      </p>
                    )}
                    {h.whisper_text && (
                      <details className="pt-1">
                        <summary className="cursor-pointer text-zinc-400 hover:text-zinc-300">
                          Whisper intermediário
                        </summary>
                        <p className="mt-1 whitespace-pre-wrap text-zinc-500">
                          {h.whisper_text}
                        </p>
                      </details>
                    )}
                    {h.sanitizer_text && (
                      <details className="pt-1">
                        <summary className="cursor-pointer text-zinc-400 hover:text-zinc-300">
                          Sanitizer intermediário
                        </summary>
                        <p className="mt-1 whitespace-pre-wrap text-zinc-500">
                          {h.sanitizer_text}
                        </p>
                      </details>
                    )}
                    {h.gemini_text && (
                      <details className="pt-1">
                        <summary className="cursor-pointer text-zinc-400 hover:text-zinc-300">
                          Gemini intermediário
                        </summary>
                        <p className="mt-1 whitespace-pre-wrap text-zinc-500">
                          {h.gemini_text}
                        </p>
                      </details>
                    )}
                  </div>
                )}

                {errors[h.id] && (
                  <p className="mt-3 flex items-center gap-1.5 text-xs text-red-400">
                    <AlertCircle className="h-3.5 w-3.5" /> {errors[h.id]}
                  </p>
                )}

                {devMode &&
                  devExpanded[h.id] &&
                  (() => {
                    const dbg = resolveDebugInfo(h);
                    return dbg ? <DebugPanel debug={dbg} /> : null;
                  })()}

                {h.evaluation && isOpen && (
                  <div className="mt-6 rounded-2xl border border-zinc-800/70 bg-zinc-950/40 p-6">
                    <div className="mb-5 flex items-center gap-2.5 border-b border-zinc-800/60 pb-4">
                      <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-coral-500/15">
                        <Sparkles className="h-4 w-4 text-coral-400" />
                      </div>
                      <div>
                        <h4 className="text-sm font-semibold text-zinc-100">
                          Avaliação de Pronúncia
                        </h4>
                        <p className="text-[11px] text-zinc-500">
                          Relatório de proficiência oral gerado pelo Gemini
                        </p>
                      </div>
                    </div>
                    <PronunciationEvaluation markdown={h.evaluation} />
                  </div>
                )}
              </Card>
            );
          })}
        </div>
      )}
    </div>
  );
}

/* ------------------------- Developer request panel ------------------------- */

function effortLabel(effort: string): string {
  return effort === "low"
    ? "Baixo"
    : effort === "medium"
      ? "Médio"
      : effort === "high"
        ? "Alto"
        : effort;
}

/** Read-only inspector for sanitizer request or product-pipeline snapshot. */
function DebugPanel({ debug }: { debug: SanitizerDebug }) {
  const isPipeline = debug.endpoint.startsWith("product-mode:");
  const reasoning = debug.reasoning_effort_applied
    ? `Aplicado — nível ${effortLabel(debug.reasoning_effort)} (parâmetro reasoning_effort enviado)`
    : debug.reasoning_enabled && !debug.reasoning_supported_by_model
      ? "Ignorado — o modelo selecionado não possui reasoning nativo"
      : debug.reasoning_enabled
        ? "Habilitado, mas não aplicado nesta requisição"
        : "Desativado — modelo usa o esforço padrão";

  return (
    <div className="mt-6 rounded-2xl border border-zinc-800/70 bg-zinc-950/40 p-6">
      <div className="mb-5 flex items-center gap-2.5 border-b border-zinc-800/60 pb-4">
        <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-coral-500/15">
          <Braces className="h-4 w-4 text-coral-400" />
        </div>
        <div>
          <h4 className="text-sm font-semibold text-zinc-100">
            {isPipeline
              ? "Debug do pipeline"
              : "Request do validador semântico"}
          </h4>
          <p className="text-[11px] text-zinc-500">
            {isPipeline
              ? "Etapas, modelos e textos intermediários deste modo de produto"
              : "Captura do que foi enviado ao chat completions da Groq"}
          </p>
        </div>
      </div>

      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <DebugField label="Modelo" value={debug.model} mono />
        <DebugField
          label="Status da resposta"
          value={debug.response_status != null ? String(debug.response_status) : "—"}
          mono
        />
        <DebugField label="Endpoint" value={debug.endpoint} mono />
        {!isPipeline && (
          <DebugField label="Temperatura" value={String(debug.temperature)} mono />
        )}
        {!isPipeline && (
          <div className="sm:col-span-2">
            <DebugField label="Reasoning" value={reasoning} />
          </div>
        )}
      </div>

      {debug.error && (
        <div className="mt-4 rounded-xl border border-red-900/30 bg-red-950/20 p-3">
          <p className="flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-wider text-red-400">
            <AlertCircle className="h-3.5 w-3.5" />{" "}
            {isPipeline ? "Fallback / aviso" : "Erro"}
          </p>
          <pre className="mt-2 whitespace-pre-wrap break-words font-mono text-xs leading-relaxed text-red-300/90">
            {debug.error}
          </pre>
        </div>
      )}

      <DebugBlock
        label={isPipeline ? "Resumo das etapas" : "System Prompt"}
        value={debug.system_prompt}
      />
      <DebugBlock
        label={isPipeline ? "Textos intermediários" : "Mensagem do Usuário"}
        value={debug.user_message}
      />
      <DebugBlock
        label={isPipeline ? "JSON do pipeline" : "JSON do Request"}
        value={debug.request_json}
      />
      {debug.response_content && (
        <DebugBlock label="Texto final" value={debug.response_content} />
      )}
      {debug.response_reasoning && (
        <DebugBlock label="Reasoning do Modelo" value={debug.response_reasoning} />
      )}
    </div>
  );
}

function DebugField({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="space-y-1">
      <p className="text-[10px] font-medium uppercase tracking-wider text-zinc-500">
        {label}
      </p>
      <p
        className={
          "break-words text-xs text-zinc-300 " + (mono ? "font-mono" : "")
        }
      >
        {value}
      </p>
    </div>
  );
}

function DebugBlock({ label, value }: { label: string; value: string }) {
  return (
    <div className="mt-4 space-y-1.5">
      <p className="text-[10px] font-medium uppercase tracking-wider text-zinc-500">
        {label}
      </p>
      <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words rounded-xl border border-zinc-800/60 bg-zinc-900/60 p-3 font-mono text-[11px] leading-relaxed text-zinc-300 scrollbar-thin">
        {value}
      </pre>
    </div>
  );
}
