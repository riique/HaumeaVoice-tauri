import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  AlertCircle,
  Braces,
  Check,
  Copy,
  FolderOpen,
  Loader2,
  MoreHorizontal,
  Pause,
  Pencil,
  Play,
  RefreshCw,
  Search,
  Sparkles,
  Trash2,
} from "lucide-react";
import { Button } from "../components/ui/Button";
import { Input } from "../components/ui/Input";
import { EmptyState, PageHeader, SkeletonRows } from "../components/ui/Surface";
import { PronunciationEvaluation } from "../components/PronunciationEvaluation";
import {
  evaluatePronunciation,
  getDevMode,
  getHistory,
  readHistoryAudio,
  revealHistoryAudio,
  retryTranscription,
  type HistoryEntry,
} from "../lib/tauri";

function productModeLabel(mode: string | null | undefined): string | null {
  if (!mode) return null;
  return ({
    "ultra-fast": "Ultrarrápido",
    "fast-accurate": "Rápido e preciso",
    precise: "Preciso",
    "ultra-precise": "Ultrapreciso",
  } as Record<string, string>)[mode] ?? mode;
}

function formatDuration(milliseconds: number | null | undefined): string | null {
  if (!milliseconds) return null;
  const seconds = Math.round(milliseconds / 1000);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

function formatEntryDate(value: string): string {
  const normalized = value.includes("T") ? value : value.replace(" ", "T");
  const parsed = new Date(normalized);
  if (Number.isNaN(parsed.getTime())) return value;
  return new Intl.DateTimeFormat("pt-BR", {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(parsed);
}

function audioMimeType(path: string | null | undefined): string {
  const extension = path?.split(".").pop()?.toLowerCase();
  return ({
    wav: "audio/wav",
    wave: "audio/wav",
    mp3: "audio/mpeg",
    m4a: "audio/mp4",
    mp4: "audio/mp4",
    aac: "audio/mp4",
    flac: "audio/flac",
    ogg: "audio/ogg",
    oga: "audio/ogg",
    webm: "audio/webm",
  } as Record<string, string>)[extension ?? ""] ?? "application/octet-stream";
}

function formatHistoryForClipboard(items: HistoryEntry[]): string {
  return items
    .filter((item) => !item.is_error && item.text.trim())
    .map((item, index) => {
      const model = item.model?.trim() || item.engine?.trim() || "Não informado";
      const pipeline = productModeLabel(item.mode) || "Legado";
      const stages = item.stages?.split(",").map((stage) => stage.trim()).filter(Boolean).join(" → ");
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

function TechnicalDetails({ entry }: { entry: HistoryEntry }) {
  const fields = [
    ["Pipeline", productModeLabel(entry.mode)],
    ["Modelo", entry.model || entry.engine],
    ["Etapas", entry.stages?.split(",").join(" → ")],
    ["Conteúdo", entry.content_type],
    ["Duração", formatDuration(entry.duration_ms)],
    ["Pipeline total", entry.total_pipeline_ms != null ? `${(entry.total_pipeline_ms / 1000).toFixed(2)} s` : null],
    ["Whisper", entry.whisper_ms != null ? `${(entry.whisper_ms / 1000).toFixed(2)} s` : null],
    ["Sanitizador", entry.sanitizer_ms != null ? `${(entry.sanitizer_ms / 1000).toFixed(2)} s` : null],
    ["Gemini", entry.gemini_generate_ms != null ? `${(entry.gemini_generate_ms / 1000).toFixed(2)} s` : null],
    ["RTF", entry.realtime_factor != null ? `${entry.realtime_factor.toFixed(2)}×` : null],
    ["Tokens", entry.total_tokens?.toString()],
    ["Transporte", entry.gemini_transport],
  ].filter((field): field is [string, string] => Boolean(field[1]));

  return (
    <div className="mt-4 rounded-[9px] bg-[#f2f2ed] p-4">
      <div className="grid grid-cols-2 gap-x-6 gap-y-3 sm:grid-cols-3">
        {fields.map(([label, value]) => (
          <div key={label} className="min-w-0">
            <div className="meta-label">{label}</div>
            <div className="mt-1 truncate font-mono text-[11px] text-[#444540]" title={value}>{value}</div>
          </div>
        ))}
      </div>
      {entry.used_fallback && (
        <p className="mt-4 rounded-[7px] bg-[#f7edd9] px-3 py-2 text-[11px] leading-4 text-[#795019]">
          Fallback utilizado{entry.fallback_reason ? `: ${entry.fallback_reason}` : "."}
        </p>
      )}
      {entry.warnings?.length ? (
        <ul className="mt-3 space-y-1 text-[11px] text-[#795019]">{entry.warnings.map((warning) => <li key={warning}>• {warning}</li>)}</ul>
      ) : null}
      {entry.debug_info && (
        <details className="mt-4">
          <summary className="cursor-pointer text-[11px] font-medium text-[#555650]">Request / resposta técnica</summary>
          <pre className="mt-3 max-h-72 overflow-auto whitespace-pre-wrap break-words rounded-[8px] bg-[#252522] p-3 font-mono text-[10px] leading-4 text-[#e8e8e2]">
            {entry.debug_info.request_json || entry.debug_info.user_message}
          </pre>
        </details>
      )}
    </div>
  );
}

export function HistoricoView() {
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [devMode, setDevMode] = useState(false);
  const [menuOpen, setMenuOpen] = useState<string | null>(null);
  const [detailsOpen, setDetailsOpen] = useState<Record<string, boolean>>({});
  const [evaluationOpen, setEvaluationOpen] = useState<Record<string, boolean>>({});
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editDraft, setEditDraft] = useState("");
  const [busyId, setBusyId] = useState<string | null>(null);
  const [loadingAudioId, setLoadingAudioId] = useState<string | null>(null);
  const [playingId, setPlayingId] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const menuRef = useRef<HTMLDivElement | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const audioUrlRef = useRef<string | null>(null);
  const activeAudioIdRef = useRef<string | null>(null);

  const releaseAudio = () => {
    const audio = audioRef.current;
    if (audio) {
      audio.pause();
      audio.removeAttribute("src");
      audio.load();
    }
    if (audioUrlRef.current) URL.revokeObjectURL(audioUrlRef.current);
    audioRef.current = null;
    audioUrlRef.current = null;
    activeAudioIdRef.current = null;
  };

  const disposeAudio = () => {
    releaseAudio();
    setPlayingId(null);
  };

  const refresh = async () => {
    try {
      setItems(await getHistory());
    } catch (error) {
      console.error("failed to load history:", error);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void refresh();
    void getDevMode().then(setDevMode).catch(console.error);
    const unlistenPromise = listen("transcription-saved", () => void refresh());
    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
      releaseAudio();
    };
  }, []);

  useEffect(() => {
    const close = (event: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) setMenuOpen(null);
    };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, []);

  const filtered = query.trim()
    ? items.filter((item) => `${item.text} ${item.error_message ?? ""}`.toLowerCase().includes(query.trim().toLowerCase()))
    : items;

  const copyText = async (id: string, text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedId(id);
      window.setTimeout(() => setCopiedId((current) => current === id ? null : current), 1500);
    } catch {
      setErrors((current) => ({ ...current, [id]: "Não foi possível copiar." }));
    }
  };

  const toggleAudio = async (entry: HistoryEntry) => {
    const currentAudio = audioRef.current;
    if (activeAudioIdRef.current === entry.id && currentAudio) {
      if (currentAudio.paused) {
        await currentAudio.play();
      } else {
        currentAudio.pause();
      }
      return;
    }

    disposeAudio();
    setLoadingAudioId(entry.id);
    setErrors((current) => {
      const next = { ...current };
      delete next[entry.id];
      return next;
    });
    try {
      const bytes = await readHistoryAudio(entry.id);
      const url = URL.createObjectURL(new Blob([bytes], { type: audioMimeType(entry.audio_path) }));
      const audio = new Audio(url);
      audioRef.current = audio;
      audioUrlRef.current = url;
      activeAudioIdRef.current = entry.id;
      audio.onplay = () => setPlayingId(entry.id);
      audio.onpause = () => setPlayingId((current) => current === entry.id ? null : current);
      audio.onended = disposeAudio;
      audio.onerror = () => {
        setErrors((current) => ({ ...current, [entry.id]: "Não foi possível reproduzir o áudio salvo." }));
        disposeAudio();
      };
      await audio.play();
    } catch (error) {
      disposeAudio();
      setErrors((current) => ({ ...current, [entry.id]: String(error) }));
    } finally {
      setLoadingAudioId((current) => current === entry.id ? null : current);
    }
  };

  const saveEdit = async (id: string) => {
    try {
      await invoke("update_history_text", { id, text: editDraft });
      setItems((current) => current.map((item) => item.id === id ? { ...item, text: editDraft, words: editDraft.trim() ? editDraft.trim().split(/\s+/).length : 0, is_error: false, error_message: null } : item));
      setEditingId(null);
    } catch (error) {
      setErrors((current) => ({ ...current, [id]: String(error) }));
    }
  };

  const retry = async (id: string) => {
    setBusyId(id);
    setMenuOpen(null);
    try {
      await retryTranscription(id);
      await refresh();
    } catch (error) {
      setErrors((current) => ({ ...current, [id]: String(error) }));
    } finally {
      setBusyId(null);
    }
  };

  const evaluate = async (entry: HistoryEntry) => {
    setMenuOpen(null);
    if (entry.evaluation) {
      setEvaluationOpen((current) => ({ ...current, [entry.id]: !current[entry.id] }));
      return;
    }
    setBusyId(entry.id);
    try {
      const evaluation = await evaluatePronunciation(entry.id);
      setItems((current) => current.map((item) => item.id === entry.id ? { ...item, evaluation } : item));
      setEvaluationOpen((current) => ({ ...current, [entry.id]: true }));
    } catch (error) {
      setErrors((current) => ({ ...current, [entry.id]: String(error) }));
    } finally {
      setBusyId(null);
    }
  };

  const remove = async (id: string) => {
    setMenuOpen(null);
    if (!window.confirm("Excluir esta transcrição e o áudio salvo?")) return;
    try {
      if (activeAudioIdRef.current === id) disposeAudio();
      await invoke("delete_history_entry", { id });
      setItems((current) => current.filter((item) => item.id !== id));
    } catch (error) {
      setErrors((current) => ({ ...current, [id]: String(error) }));
    }
  };

  const copyAll = async () => {
    const text = formatHistoryForClipboard(items);
    if (text) await navigator.clipboard.writeText(text);
  };

  const clearAll = async () => {
    if (!window.confirm("Limpar todo o histórico e remover os áudios salvos?")) return;
    disposeAudio();
    await invoke("clear_history");
    setItems([]);
  };

  return (
    <div className="space-y-7">
      <PageHeader title="Histórico" description="Suas transcrições salvas, ordenadas por data." />

      <div className="flex flex-wrap items-center gap-2">
        <div className="relative min-w-64 flex-1">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[#8b8c85]" aria-hidden />
          <Input aria-label="Buscar transcrições" placeholder="Buscar transcrições…" value={query} onChange={(event) => setQuery(event.target.value)} className="pl-9" />
        </div>
        <Button size="sm" disabled={!items.some((item) => !item.is_error && item.text.trim())} onClick={() => void copyAll()}><Copy className="h-3.5 w-3.5" aria-hidden />Copiar todas</Button>
        <Button size="sm" variant="danger" disabled={!items.length} onClick={() => void clearAll()}><Trash2 className="h-3.5 w-3.5" aria-hidden />Limpar</Button>
      </div>

      {loading ? <div className="surface"><SkeletonRows count={5} /></div> : filtered.length === 0 ? (
        <div className="surface"><EmptyState title={query ? "Nenhum resultado" : "Histórico vazio"} description={query ? "Tente buscar por outro trecho." : "Suas próximas transcrições aparecerão aqui."} /></div>
      ) : (
        <div className="isolate divide-y divide-line border-y border-line">
          {filtered.map((entry) => {
            const isError = Boolean(entry.is_error);
            const hasAudio = Boolean(entry.audio_path);
            const isBusy = busyId === entry.id;
            return (
              <article key={entry.id} className={`relative px-2 py-5 ${menuOpen === entry.id ? "z-30" : "z-0"}`}>
                <div className="grid grid-cols-[116px_minmax(0,1fr)_auto] gap-5 max-[860px]:grid-cols-[minmax(0,1fr)_auto]">
                  <div className="text-[11px] leading-5 text-muted max-[860px]:col-span-2">
                    <div>{formatEntryDate(entry.date)}</div>
                    {formatDuration(entry.duration_ms) && <div>{formatDuration(entry.duration_ms)}</div>}
                  </div>
                  <div className="min-w-0">
                    {editingId === entry.id ? (
                      <div className="space-y-2">
                        <textarea autoFocus value={editDraft} onChange={(event) => setEditDraft(event.target.value)} className="min-h-28 w-full rounded-[9px] border border-line bg-white px-3 py-2 text-[13px] leading-5 text-ink outline-none" aria-label="Editar transcrição" />
                        <div className="flex gap-2"><Button size="sm" variant="primary" onClick={() => void saveEdit(entry.id)}>Salvar</Button><Button size="sm" onClick={() => setEditingId(null)}>Cancelar</Button></div>
                      </div>
                    ) : isError ? (
                      <div className="flex items-start gap-2 text-[#9f2720]">
                        <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
                        <div><h3 className="text-[13px] font-medium">Falha na transcrição</h3><p className="mt-1 text-[12px] leading-5">{entry.error_message || "Não foi possível processar o áudio."}</p></div>
                      </div>
                    ) : (
                      <>
                        <p className="line-clamp-2 text-[13px] leading-5 text-[#343530]">{entry.text}</p>
                        <div className="mt-2 flex flex-wrap items-center gap-x-2 text-[10px] text-muted">
                          <span>{entry.words} palavras</span><span>·</span><span>{productModeLabel(entry.mode) || entry.engine}</span>
                          {entry.used_fallback && <><span>·</span><span className="text-[#80551a]">Fallback</span></>}
                          {devMode && entry.model && <><span>·</span><span className="font-mono">{entry.model}</span></>}
                        </div>
                      </>
                    )}
                  </div>
                  <div className="flex items-start gap-1">
                    {hasAudio && <button className="icon-button" disabled={loadingAudioId === entry.id} onClick={() => void toggleAudio(entry)} aria-label={playingId === entry.id ? "Pausar áudio" : "Reproduzir áudio"} title={playingId === entry.id ? "Pausar áudio" : "Reproduzir áudio"}>{loadingAudioId === entry.id ? <Loader2 className="h-4 w-4 animate-spin" /> : playingId === entry.id ? <Pause className="h-4 w-4" /> : <Play className="h-4 w-4" />}</button>}
                    {!isError && <button className="icon-button" onClick={() => void copyText(entry.id, entry.text)} aria-label="Copiar transcrição" title="Copiar">{copiedId === entry.id ? <Check className="h-4 w-4 text-[#25613f]" /> : <Copy className="h-4 w-4" />}</button>}
                    {!isError && <button className="icon-button" onClick={() => { setEditingId(entry.id); setEditDraft(entry.text); }} aria-label="Editar transcrição" title="Editar"><Pencil className="h-4 w-4" /></button>}
                    {hasAudio && <button className="icon-button" disabled={isBusy} onClick={() => void retry(entry.id)} aria-label="Retranscrever" title="Retranscrever">{isBusy ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}</button>}
                    <div className="relative" ref={menuOpen === entry.id ? menuRef : undefined}>
                      <button className="icon-button" onClick={() => setMenuOpen((current) => current === entry.id ? null : entry.id)} aria-label="Mais ações" aria-expanded={menuOpen === entry.id}><MoreHorizontal className="h-4 w-4" /></button>
                      {menuOpen === entry.id && (
                        <div className="menu-panel z-50 w-52" role="menu">
                          {hasAudio && <button className="menu-item" role="menuitem" onClick={() => { setMenuOpen(null); void revealHistoryAudio(entry.id).catch((error) => setErrors((current) => ({ ...current, [entry.id]: String(error) }))); }}><FolderOpen className="h-4 w-4" />Mostrar áudio</button>}
                          {hasAudio && !isError && <button className="menu-item" role="menuitem" onClick={() => void evaluate(entry)}><Sparkles className="h-4 w-4" />{entry.evaluation ? "Ver pronúncia" : "Avaliar pronúncia"}</button>}
                          <button className="menu-item" role="menuitem" onClick={() => { setDetailsOpen((current) => ({ ...current, [entry.id]: !current[entry.id] })); setMenuOpen(null); }}><Braces className="h-4 w-4" />{detailsOpen[entry.id] ? "Ocultar detalhes" : "Detalhes"}</button>
                          <button className="menu-item text-[#a72a21]" role="menuitem" onClick={() => void remove(entry.id)}><Trash2 className="h-4 w-4" />Excluir</button>
                        </div>
                      )}
                    </div>
                  </div>
                </div>
                {errors[entry.id] && <p className="mt-3 rounded-[8px] bg-[#fff1ef] px-3 py-2 text-[11px] text-[#9f2720]" role="alert">{errors[entry.id]}</p>}
                {detailsOpen[entry.id] && <TechnicalDetails entry={entry} />}
                {evaluationOpen[entry.id] && entry.evaluation && <div className="mt-5"><PronunciationEvaluation markdown={entry.evaluation} /></div>}
              </article>
            );
          })}
        </div>
      )}
    </div>
  );
}
