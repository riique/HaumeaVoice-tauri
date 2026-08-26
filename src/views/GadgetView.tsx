import { useCallback, useEffect, useRef, useState } from "react";
import { AlertCircle, Check, Mic, RefreshCw, Square, X } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import {
  cancelRecording,
  getRecordingState,
  getShortcuts,
  getWidgetPreferences,
  retryTranscription,
  retryTranscriptionWithFallback,
  setGadgetHitRect,
  setGadgetVisualState,
  toggleRecordingState,
  type HistoryEntry,
  type PipelineProgressEvent,
  type WidgetPreferences,
  type WidgetVisibilityMode,
} from "../lib/tauri";
import {
  GADGET_STATES,
  restState,
  stateAfterTimeout,
  type GadgetState,
} from "../gadget/machine";

const BAR_COUNT = 18;
const WAVE_WEIGHTS = [
  0.18, 0.28, 0.42, 0.6, 0.78, 0.92, 0.7, 0.48, 0.82,
  1, 0.76, 0.58, 0.88, 0.72, 0.54, 0.38, 0.26, 0.18,
];

function shortcutLabel(value: string): string {
  return value.replace("CommandOrControl", "Ctrl").replace("Control", "Ctrl").split("+").join(" + ");
}

export function GadgetApp() {
  const [state, setState] = useState<GadgetState>("hidden");
  const [visibilityMode, setVisibilityMode] = useState<WidgetVisibilityMode>("auto");
  const [audioLevel, setAudioLevel] = useState(0);
  const [shortcut, setShortcut] = useState("Ctrl + B");
  const [failure, setFailure] = useState<{ id: string; message: string; canRetry: boolean } | null>(null);
  const [retrying, setRetrying] = useState(false);
  const [progressMessage, setProgressMessage] = useState<string | null>(null);
  const pillRef = useRef<HTMLDivElement>(null);
  const stateRef = useRef<GadgetState>("hidden");
  const modeRef = useRef<WidgetVisibilityMode>("auto");
  const lastHitRectRef = useRef("");

  const transition = useCallback((next: GadgetState) => {
    if (stateRef.current === next) return;
    stateRef.current = next;
    setState(next);
    void setGadgetVisualState(next).catch((error) => console.error("set_gadget_visual_state failed:", error));
  }, []);

  const transitionToRest = useCallback(() => {
    setAudioLevel(0);
    transition(restState(modeRef.current));
  }, [transition]);

  useEffect(() => {
    let mounted = true;
    void Promise.all([getRecordingState(), getWidgetPreferences(), getShortcuts()])
      .then(([recording, preferences, shortcuts]) => {
        if (!mounted) return;
        modeRef.current = preferences.visibility_mode;
        setVisibilityMode(preferences.visibility_mode);
        setShortcut(shortcutLabel(shortcuts.toggle));
        transition(recording ? "recording" : restState(preferences.visibility_mode));
      })
      .catch((error) => console.error("gadget bootstrap failed:", error));

    const subscriptions: Array<Promise<() => void>> = [
      listen("recording-initializing", () => {
        setFailure(null);
        setRetrying(false);
        setProgressMessage(null);
        setAudioLevel(0);
        transition("appearing");
      }),
      listen("recording-started", () => {
        setFailure(null);
        setAudioLevel(0);
        transition("recording");
      }),
      listen("recording-stopped", () => {
        setAudioLevel(0);
        transition("stopping");
      }),
      listen("recording-cancelled", transitionToRest),
      listen<number>("audio-level", (event) => {
        const level = Number.isFinite(event.payload) ? Math.max(0, Math.min(1, event.payload)) : 0;
        setAudioLevel((previous) => Math.abs(previous - level) < 0.005 ? previous : level);
      }),
      listen<boolean>("transcribing", (event) => {
        if (event.payload) {
          transition("processing");
        } else if (["processing", "processing_long", "stopping"].includes(stateRef.current)) {
          setFailure({ id: "", message: "Nenhuma fala foi detectada na gravação.", canRetry: false });
          transition("error");
        }
      }),
      listen<HistoryEntry>("transcription-saved", (event) => {
        const entry = event.payload;
        if (entry.source && entry.source !== "mic") return;
        setRetrying(false);
        setProgressMessage(null);
        if (entry.is_error) {
          setFailure({
            id: entry.id,
            message: entry.error_message || "Não foi possível transcrever.",
            canRetry: Boolean(entry.audio_path),
          });
          transition("error");
        } else {
          setFailure(null);
          transition("success");
        }
      }),
      listen<PipelineProgressEvent>("pipeline-progress", (event) => {
        const progress = event.payload;
        if (progress.kind === "complete") {
          setProgressMessage(null);
          return;
        }
        if (progress.message) setProgressMessage(progress.message);
        if (progress.kind === "fallback_started") transition("processing_long");
      }),
      listen<WidgetPreferences>("widget-preferences-changed", (event) => {
        const mode = event.payload.visibility_mode;
        modeRef.current = mode;
        setVisibilityMode(mode);
        if (["hidden", "idle", "hover"].includes(stateRef.current)) transition(restState(mode));
      }),
    ];

    return () => {
      mounted = false;
      subscriptions.forEach((subscription) => void subscription.then((unlisten) => unlisten()));
    };
  }, [transition, transitionToRest]);

  useEffect(() => {
    const timeout = GADGET_STATES[state].timeoutMs;
    if (timeout === null) return;
    const timer = window.setTimeout(() => {
      const next = stateAfterTimeout(stateRef.current, modeRef.current);
      if (next) transition(next);
    }, timeout);
    return () => window.clearTimeout(timer);
  }, [state, transition]);

  const reportHitRect = useCallback(() => {
    const element = pillRef.current;
    if (!element || GADGET_STATES[stateRef.current].visibility === "hidden") return;
    const rect = element.getBoundingClientRect();
    const next = { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
    const serialized = JSON.stringify(next);
    if (serialized === lastHitRectRef.current) return;
    lastHitRectRef.current = serialized;
    void setGadgetHitRect(next).catch(() => undefined);
  }, []);

  useEffect(() => {
    reportHitRect();
    const element = pillRef.current;
    if (!element || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(reportHitRect);
    observer.observe(element);
    return () => observer.disconnect();
  }, [state, reportHitRect]);

  const startOrStop = async () => {
    if (["idle", "hover"].includes(stateRef.current)) transition("appearing");
    if (stateRef.current === "recording") transition("stopping");
    try {
      await toggleRecordingState();
    } catch (error) {
      setFailure({ id: "", message: typeof error === "string" ? error : "Não foi possível acessar o microfone.", canRetry: false });
      transition("error");
    }
  };

  const cancel = async () => {
    try {
      await cancelRecording();
    } catch (error) {
      console.error("cancel_recording failed:", error);
    }
  };

  const retry = async () => {
    if (!failure?.canRetry || retrying) return;
    setRetrying(true);
    transition("processing_long");
    try {
      await retryTranscription(failure.id);
    } catch (error) {
      setRetrying(false);
      setFailure((current) => current ? { ...current, message: typeof error === "string" ? error : "A retranscrição falhou." } : current);
      transition("error");
    }
  };

  const useFallback = async () => {
    if (!failure?.canRetry || retrying) return;
    setRetrying(true); setProgressMessage("Usando rota alternativa…"); transition("processing_long");
    try { await retryTranscriptionWithFallback(failure.id); }
    catch (error) {
      setRetrying(false);
      setFailure((current) => current ? { ...current, message: typeof error === "string" ? error : "O fallback falhou." } : current);
      transition("error");
    }
  };

  if (state === "hidden") return null;

  return (
    <div className="gadget-window">
      <div
        ref={pillRef}
        className={`gadget-stage gadget-stage--${GADGET_STATES[state].animation}`}
        onPointerEnter={() => {
          if (stateRef.current === "idle" && visibilityMode === "always") transition("hover");
        }}
        onPointerLeave={() => {
          if (stateRef.current === "hover") transition("idle");
        }}
        aria-live="polite"
        aria-label={GADGET_STATES[state].accessibleLabel}
      >
        <GadgetPill
          state={state}
          level={audioLevel}
          shortcut={shortcut}
          failure={failure}
          retrying={retrying}
          progressMessage={progressMessage}
          onToggle={() => void startOrStop()}
          onCancel={() => void cancel()}
          onRetry={() => void retry()}
          onFallback={() => void useFallback()}
        />
      </div>
    </div>
  );
}

interface GadgetPillProps {
  state: Exclude<GadgetState, "hidden">;
  level: number;
  shortcut: string;
  failure: { id: string; message: string; canRetry: boolean } | null;
  retrying: boolean;
  progressMessage: string | null;
  onToggle: () => void;
  onCancel: () => void;
  onRetry: () => void;
  onFallback: () => void;
}

function GadgetPill({ state, level, shortcut, failure, retrying, progressMessage, onToggle, onCancel, onRetry, onFallback }: GadgetPillProps) {
  if (state === "idle") {
    return <button type="button" onClick={onToggle} className="haumea-bar haumea-bar--idle" aria-label={`Iniciar ditado. ${shortcut}`}><MiniWave /></button>;
  }
  if (state === "hover") {
    return (
      <button type="button" onClick={onToggle} className="haumea-bar haumea-bar--hover" aria-label={`Iniciar ditado. ${shortcut}`}>
        <MiniWave /><Mic className="h-[15px] w-[15px]" aria-hidden /><span className="haumea-bar__shortcut">{shortcut}</span>
      </button>
    );
  }
  if (state === "recording") {
    return (
      <div className="haumea-bar haumea-bar--recording" role="status">
        <BarButton label="Cancelar e descartar" onClick={onCancel}><X className="h-[17px] w-[17px]" strokeWidth={2} /></BarButton>
        <Waveform level={level} />
        <BarButton label="Parar e transcrever" onClick={onToggle} primary><Square className="h-[11px] w-[11px] fill-current" strokeWidth={1.5} /></BarButton>
      </div>
    );
  }
  if (state === "processing" || state === "processing_long") {
    return (
      <div className={`haumea-bar ${state === "processing" ? "haumea-bar--processing" : "haumea-bar--processing-long"}`} role="status">
        <ProcessingDots />
        {(state === "processing_long" || progressMessage) && <span className="haumea-bar__status">{retrying ? "Tentando novamente…" : progressMessage || "Transcrevendo…"}</span>}
      </div>
    );
  }
  if (state === "success") {
    return <div className="haumea-bar haumea-bar--success" role="status"><Check className="h-[17px] w-[17px]" strokeWidth={2.2} aria-hidden /></div>;
  }
  if (state === "error") {
    return (
      <div className="haumea-bar haumea-bar--error" role="alert" title={failure?.message}>
        <AlertCircle className="h-4 w-4 shrink-0 text-[#ffaaa3]" aria-hidden />
        <span className="haumea-bar__error-text">Falha na transcrição</span>
        {failure?.canRetry ? (
          <><button type="button" onClick={onRetry} disabled={retrying} className="haumea-bar__retry" aria-label="Tentar transcrever o áudio salvo novamente">
            <RefreshCw className={`h-3.5 w-3.5 ${retrying ? "animate-spin" : ""}`} aria-hidden /><span>Tentar novamente</span>
          </button><button type="button" onClick={onFallback} disabled={retrying} className="haumea-bar__retry" aria-label="Usar uma rota alternativa de transcrição"><span>Usar fallback</span></button></>
        ) : (
          <button type="button" onClick={onCancel} className="haumea-bar__dismiss" aria-label="Dispensar erro"><X className="h-3.5 w-3.5" aria-hidden /></button>
        )}
      </div>
    );
  }
  return <div className="haumea-bar haumea-bar--activity" role="status"><ActivityMark settled={state === "stopping"} /></div>;
}

function BarButton({ label, onClick, primary = false, children }: { label: string; onClick: () => void; primary?: boolean; children: React.ReactNode }) {
  return <button type="button" onClick={onClick} className={`haumea-bar__control ${primary ? "haumea-bar__control--primary" : ""}`} aria-label={label}>{children}</button>;
}

function ActivityMark({ settled = false }: { settled?: boolean }) {
  return <span className={`activity-mark ${settled ? "activity-mark--settled" : ""}`} aria-hidden><span /><span /><span /></span>;
}

function ProcessingDots() {
  return <span className="processing-dots" aria-hidden><span /><span /><span /></span>;
}

function MiniWave() {
  return <span className="mini-wave" aria-hidden>{[5, 10, 15, 9, 5].map((height, index) => <span key={index} style={{ height }} />)}</span>;
}

function Waveform({ level }: { level: number }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const targetRef = useRef(0);
  const envelopeRef = useRef(0);

  useEffect(() => {
    targetRef.current = Math.min(1, Math.max(0, level));
  }, [level]);

  useEffect(() => {
    let frame = 0;
    let phase = 0;
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const tick = () => {
      const target = targetRef.current;
      const current = envelopeRef.current;
      envelopeRef.current += (target - current) * (target > current ? 0.42 : 0.12);
      const envelope = envelopeRef.current < 0.008 ? 0 : envelopeRef.current;
      if (!reduceMotion && envelope > 0) phase += 0.11 + envelope * 0.035;
      Array.from(containerRef.current?.children ?? []).forEach((child, index) => {
        const weight = WAVE_WEIGHTS[index] ?? 0.3;
        const organic = envelope > 0 && !reduceMotion ? 0.92 + Math.sin(phase + index * 0.73) * 0.09 : 1;
        const height = 3 + envelope * 25 * weight * organic;
        const bar = child as HTMLElement;
        bar.style.height = `${Math.max(3, Math.min(28, height))}px`;
        bar.style.opacity = `${0.42 + envelope * 0.58}`;
      });
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, []);

  return <div ref={containerRef} className="live-wave" aria-hidden>{Array.from({ length: BAR_COUNT }).map((_, index) => <span key={index} />)}</div>;
}

/** Deterministic visual QA surface; reachable only from the Vite dev build. */
export function GadgetPreviewApp() {
  const [level, setLevel] = useState(0.66);
  const states: Array<Exclude<GadgetState, "hidden">> = ["idle", "hover", "initializing", "recording", "processing", "processing_long", "success", "error"];
  return (
    <main className="gadget-preview">
      <header><h1>Haumea Bar — visual QA</h1><label>RMS<input type="range" min="0" max="1" step="0.01" value={level} onChange={(event) => setLevel(Number(event.target.value))} /></label></header>
      <div className="gadget-preview__grid">
        {states.map((previewState) => (
          <figure key={previewState}>
            <div className="gadget-preview__stage">
              <GadgetPill state={previewState} level={level} shortcut="Ctrl + B" failure={{ id: "preview", message: "Falha na transcrição", canRetry: true }} retrying={false} progressMessage={previewState === "processing_long" ? "Groq indisponível · usando Deepgram" : null} onToggle={() => undefined} onCancel={() => undefined} onRetry={() => undefined} onFallback={() => undefined} />
            </div>
            <figcaption>{previewState}</figcaption>
          </figure>
        ))}
      </div>
    </main>
  );
}
