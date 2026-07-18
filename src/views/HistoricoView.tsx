import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Search, Trash2, Sparkles, Loader2, ChevronDown, AlertCircle, RefreshCw, Braces } from "lucide-react";
import { Button } from "../components/ui/Button";
import { Card } from "../components/ui/Card";
import { Input } from "../components/ui/Input";
import { PronunciationEvaluation } from "../components/PronunciationEvaluation";
import { evaluatePronunciation, getDevMode, type SanitizerDebug } from "../lib/tauri";

interface HistoryEntry {
  id: string;
  date: string;
  words: number;
  engine: string;
  text: string;
  audio_path?: string | null;
  evaluation?: string | null;
  duration_ms?: number;
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
}

function deepgramModeLabel(mode: string | null | undefined): string | null {
  if (!mode) return null;
  if (mode === "streaming_final") return "DG streaming";
  if (mode === "batch") return "DG batch";
  return mode;
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

  const filtered = query.trim()
    ? items.filter((h) =>
        h.text.toLowerCase().includes(query.trim().toLowerCase()) ||
        (h.error_message && h.error_message.toLowerCase().includes(query.trim().toLowerCase()))
      )
    : items;

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
      <div className="flex items-center gap-3">
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
          variant="danger"
          className="gap-2"
          disabled={items.length === 0}
          onClick={handleClearAll}
        >
          <Trash2 className="h-4 w-4" />
          Limpar Tudo
        </Button>
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
                        {deepgramModeLabel(h.deepgram_mode) && (
                          <>
                            <span className="text-xs text-zinc-700">•</span>
                            <span className="inline-flex items-center rounded bg-coral-500/10 px-2 py-0.5 text-[10px] font-medium text-coral-400 border border-coral-500/20">
                              {deepgramModeLabel(h.deepgram_mode)}
                            </span>
                          </>
                        )}
                        {h.transcription_latency_ms !== undefined && h.transcription_latency_ms !== null && h.sanitizer_latency_ms !== undefined && h.sanitizer_latency_ms !== null ? (
                          <>
                            <span className="text-xs text-zinc-700">•</span>
                            <span className="text-xs text-zinc-500">
                              {(h.transcription_latency_ms / 1000).toFixed(2)}s motor + {(h.sanitizer_latency_ms / 1000).toFixed(2)}s sanitizador
                            </span>
                          </>
                        ) : (
                          h.latency_ms !== undefined && h.latency_ms > 0 && (
                            <>
                              <span className="text-xs text-zinc-700">•</span>
                              <span className="text-xs text-zinc-500">
                                {(h.latency_ms / 1000).toFixed(2)}s latência
                              </span>
                            </>
                          )
                        )}
                        {h.realtime_factor !== undefined && h.realtime_factor !== null && h.realtime_factor > 0 && (
                          <>
                            <span className="text-xs text-zinc-700">•</span>
                            <span
                              className="text-xs text-zinc-500"
                              title="Fator de tempo real: latência do motor ÷ duração do áudio. Menor que 1,0 = mais rápido que tempo real."
                            >
                              RTF {h.realtime_factor.toFixed(2)}×
                            </span>
                          </>
                        )}
                        {h.transcription_throughput !== undefined && h.transcription_throughput !== null && h.transcription_throughput > 0 && (
                          <>
                            <span className="text-xs text-zinc-700">•</span>
                            <span className="text-xs text-zinc-500">
                              {h.transcription_throughput.toFixed(1)} tok/s (motor)
                            </span>
                          </>
                        )}
                        {h.sanitizer_throughput !== undefined && h.sanitizer_throughput !== null && h.sanitizer_throughput > 0 && (
                          <>
                            <span className="text-xs text-zinc-700">•</span>
                            <span className="text-xs text-zinc-500">
                              {h.sanitizer_throughput.toFixed(1)} tok/s (sanitizador)
                            </span>
                          </>
                        )}
                        {h.transcription_throughput === undefined && h.throughput !== undefined && h.throughput > 0 && (
                          <>
                            <span className="text-xs text-zinc-700">•</span>
                            <span className="text-xs text-zinc-500">
                              {h.throughput.toFixed(1)} tok/s
                            </span>
                          </>
                        )}
                      </>
                    )}
                  </div>

                  <div className="flex items-center gap-2 shrink-0">
                    {devMode && h.debug_info && (
                      <Button
                        variant="secondary"
                        className="gap-2 px-3 py-1.5 text-xs"
                        onClick={() =>
                          setDevExpanded((s) => ({ ...s, [h.id]: !s[h.id] }))
                        }
                      >
                        <Braces className="h-3.5 w-3.5 text-coral-400" />
                        {devExpanded[h.id] ? "Ocultar Request" : "Ver Request"}
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
                          {isRetrying ? "Retentando..." : "Retentar"}
                        </Button>
                      )
                    ) : (
                      hasAudio && (
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
                            ? "Avaliando..."
                            : h.evaluation
                              ? isOpen
                                ? "Ocultar avaliação"
                                : "Ver avaliação"
                              : "Avaliar Pronúncia"}
                        </Button>
                      )
                    )}
                  </div>
                </div>

                {isError ? (
                  <div className="rounded-xl border border-red-900/30 bg-red-950/20 p-4">
                    <div className="flex gap-2">
                      <AlertCircle className="h-5 w-5 text-red-400 shrink-0 mt-0.5" />
                      <div className="space-y-1">
                        <h4 className="text-sm font-semibold text-red-200">Falha na Transcrição</h4>
                        <p className="text-sm leading-relaxed text-red-300/80 whitespace-pre-line">
                          {h.error_message || "Ocorreu um erro desconhecido ao processar o áudio."}
                        </p>
                      </div>
                    </div>
                  </div>
                ) : (
                  <p className="text-[15px] leading-relaxed text-zinc-300">
                    {h.text}
                  </p>
                )}

                {errors[h.id] && (
                  <p className="mt-3 flex items-center gap-1.5 text-xs text-red-400">
                    <AlertCircle className="h-3.5 w-3.5" /> {errors[h.id]}
                  </p>
                )}

                {devMode && h.debug_info && devExpanded[h.id] && (
                  <DebugPanel debug={h.debug_info} />
                )}

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

/** Read-only, monospace inspector for the sanitizer request captured on an
 *  entry. Rendered inside the card when developer mode is on. */
function DebugPanel({ debug }: { debug: SanitizerDebug }) {
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
            Request do Validador Semântico
          </h4>
          <p className="text-[11px] text-zinc-500">
            Captura exata do que foi enviado ao endpoint de chat do Groq
          </p>
        </div>
      </div>

      {/* Parâmetros principais */}
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <DebugField label="Modelo" value={debug.model} mono />
        <DebugField
          label="Status da resposta"
          value={debug.response_status != null ? String(debug.response_status) : "—"}
          mono
        />
        <DebugField label="Endpoint" value={debug.endpoint} mono />
        <DebugField label="Temperatura" value={String(debug.temperature)} mono />
        <div className="sm:col-span-2">
          <DebugField label="Reasoning" value={reasoning} />
        </div>
      </div>

      {debug.error && (
        <div className="mt-4 rounded-xl border border-red-900/30 bg-red-950/20 p-3">
          <p className="flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-wider text-red-400">
            <AlertCircle className="h-3.5 w-3.5" /> Erro
          </p>
          <pre className="mt-2 whitespace-pre-wrap break-words font-mono text-xs leading-relaxed text-red-300/90">
            {debug.error}
          </pre>
        </div>
      )}

      <DebugBlock label="System Prompt" value={debug.system_prompt} />
      <DebugBlock label="Mensagem do Usuário" value={debug.user_message} />
      <DebugBlock label="JSON do Request" value={debug.request_json} />
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
