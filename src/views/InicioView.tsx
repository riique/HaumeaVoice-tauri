import { useEffect, useRef, useState, type ComponentType } from "react";
import {
  Circle,
  Mic,
  FileText,
  Type,
  AlignLeft,
  Clock,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { Card } from "../components/ui/Card";
import { KbdCombo } from "../components/ui/Kbd";
import {
  getHistory,
  getRecordingState,
  getRecordingElapsed,
  onRecordingEvent,
  toggleRecordingState,
  type HistoryEntry,
} from "../lib/tauri";

/** Formats an elapsed millisecond count as `MM:SS`. */
function formatClock(ms: number): string {
  const totalSec = Math.floor(ms / 1000);
  const m = Math.floor(totalSec / 60);
  const s = totalSec % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

/** Aggregated usage metrics derived from the full transcription history. */
interface Stats {
  transcriptions: number;
  recordings: number;
  words: number;
  avgWordsPerSentence: number;
  durationMs: number;
}

/** Computes the dashboard metrics from the persisted history entries. */
function computeStats(entries: HistoryEntry[]): Stats {
  let words = 0;
  let sentences = 0;
  let recordings = 0;
  let durationMs = 0;

  for (const e of entries) {
    words += e.words ?? 0;
    durationMs += e.duration_ms ?? 0;
    if (e.source === "mic") recordings += 1;
    const s = (e.text ?? "")
      .split(/[.!?…]+/)
      .map((t) => t.trim())
      .filter(Boolean).length;
    sentences += Math.max(s, e.text?.trim() ? 1 : 0);
  }

  return {
    transcriptions: entries.length,
    recordings,
    words,
    avgWordsPerSentence: sentences > 0 ? words / sentences : 0,
    durationMs,
  };
}

/** Formats a millisecond span as a compact human label (e.g. `1h 04m`). */
function formatDuration(ms: number): string {
  const totalSec = Math.round(ms / 1000);
  if (totalSec <= 0) return "0s";
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  const s = totalSec % 60;
  if (h > 0) return `${h}h ${String(m).padStart(2, "0")}m`;
  if (m > 0) return `${m}m ${String(s).padStart(2, "0")}s`;
  return `${s}s`;
}

/** Formats large counts compactly (1.2k, 3.4M). */
function formatCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(n);
}

export function InicioView() {
  const [recording, setRecording] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [stats, setStats] = useState<Stats>({
    transcriptions: 0,
    recordings: 0,
    words: 0,
    avgWordsPerSentence: 0,
    durationMs: 0,
  });
  const tickRef = useRef<number | null>(null);

  // Load the usage metrics and keep them fresh whenever a new transcription is
  // persisted (mic capture or file upload both emit `transcription-saved`).
  useEffect(() => {
    let mounted = true;
    const refresh = () =>
      getHistory()
        .then((h) => {
          if (mounted) setStats(computeStats(h));
        })
        .catch((e) => console.error("Failed to load history stats:", e));

    refresh();
    const unlisten = listen("transcription-saved", refresh);
    return () => {
      mounted = false;
      unlisten.then((u) => u());
    };
  }, []);

  // Sync local state with the backend on mount, restoring elapsed time from
  // the backend so the timer survives navigating between views.
  useEffect(() => {
    let mounted = true;

    (async () => {
      try {
        const [active, ms] = await Promise.all([
          getRecordingState(),
          getRecordingElapsed(),
        ]);
        if (mounted) {
          setRecording(active);
          if (active && ms > 0) setElapsed(ms);
        }
      } catch {
        // Outside of Tauri — keep the local-only behaviour.
      }
    })();

    const unlistenPromise = onRecordingEvent((type) => {
      if (type === "recording-started") {
        setRecording(true);
        setElapsed(0);
      } else if (
        type === "recording-stopped" ||
        type === "recording-cancelled"
      ) {
        setRecording(false);
        setElapsed(0);
      }
    });

    return () => {
      mounted = false;
      if (tickRef.current !== null) {
        clearInterval(tickRef.current);
        tickRef.current = null;
      }
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  // Drive the elapsed-time counter locally while recording. When the component
  // re-mounts mid-recording the elapsed state already carries the restored
  // value from the backend, so the interval just increments from there.
  useEffect(() => {
    if (recording) {
      tickRef.current = window.setInterval(
        () => setElapsed((e) => e + 1000),
        1000,
      );
    } else if (tickRef.current !== null) {
      clearInterval(tickRef.current);
      tickRef.current = null;
    }
    // Always clear on cleanup. Switching sidebar tabs (App.tsx unmounts this
    // view wholesale) while recording used to abandon the 1s interval onto a
    // dead React root, leaking the timer and calling setState on an unmounted
    // component. Under StrictMode it stacked twice.
    return () => {
      if (tickRef.current !== null) {
        clearInterval(tickRef.current);
        tickRef.current = null;
      }
    };
  }, [recording]);

  const handleToggle = async () => {
    try {
      const next = await toggleRecordingState();
      setRecording(next);
      if (!next) setElapsed(0);
    } catch (e) {
      console.error("failed to toggle recording:", e);
    }
  };

  return (
    <div className="space-y-10">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight text-zinc-100">
          Início
        </h1>
        <p className="mt-1 text-sm text-zinc-500">
          Dite e o Haumea Voice transcreve para a área de transferência.
        </p>
      </header>

      {/* Hub de Gravacao - card elevado proeminente */}
      <Card variant="hub" className="mx-auto mt-10 max-w-2xl px-16 py-14">
        <div className="flex flex-col items-center gap-6 text-center">
          {/* Cronometro - inset mais escuro dentro do card elevado */}
          <div className="rounded-2xl border border-zinc-800 bg-zinc-950/60 px-12 py-6">
            <div className="font-mono text-6xl font-light tabular-nums text-zinc-100">
              {formatClock(elapsed)}
            </div>
          </div>

          {/* Status */}
          <div className="flex items-center gap-2 text-sm text-zinc-500">
            <Circle
              className={
                "h-2 w-2 " +
                (recording
                  ? "fill-coral-500 text-coral-500"
                  : "text-zinc-600")
              }
            />
            {recording ? "Gravando..." : "Aguardando Gravação..."}
          </div>

          {/* Botao Gravar */}
          <button
            onClick={handleToggle}
            className={
              "group mt-2 flex h-24 w-24 items-center justify-center rounded-full transition-all duration-300 " +
              (recording
                ? "bg-coral-600 scale-105 shadow-glow-coral"
                : "bg-coral-500 hover:scale-105 hover:shadow-glow-coral")
            }
          >
            {recording ? (
              <span className="h-6 w-6 rounded-sm bg-white" />
            ) : (
              <Mic className="h-9 w-9 text-white" />
            )}
          </button>

          {/* Atalhos */}
          <div className="flex items-center gap-3 text-xs text-zinc-500">
            <span>Iniciar Gravação</span>
            <KbdCombo keys={["Ctrl", "B"]} />
          </div>
        </div>
      </Card>

      {/* Estatísticas de uso */}
      <section className="mx-auto max-w-5xl">
        <h2 className="mb-4 text-xs font-medium uppercase tracking-wider text-zinc-500">
          Suas estatísticas
        </h2>
        <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-5">
          <StatCard
            Icon={FileText}
            label="Transcrições"
            value={formatCount(stats.transcriptions)}
          />
          <StatCard
            Icon={Mic}
            label="Gravações"
            value={formatCount(stats.recordings)}
          />
          <StatCard
            Icon={Type}
            label="Palavras transcritas"
            value={formatCount(stats.words)}
          />
          <StatCard
            Icon={AlignLeft}
            label="Média palavras/frase"
            value={
              stats.avgWordsPerSentence
                ? stats.avgWordsPerSentence.toFixed(1)
                : "0"
            }
          />
          <StatCard
            Icon={Clock}
            label="Tempo de áudio"
            value={formatDuration(stats.durationMs)}
          />
        </div>
      </section>
    </div>
  );
}

/** A single metric tile used in the Início dashboard. */
function StatCard({
  Icon,
  label,
  value,
}: {
  Icon: ComponentType<{ className?: string }>;
  label: string;
  value: string;
}) {
  return (
    <Card className="group flex flex-col gap-3 p-5 transition-colors duration-200 hover:border-zinc-700">
      <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-coral-500/10 text-coral-400 transition-colors duration-200 group-hover:bg-coral-500/15">
        <Icon className="h-5 w-5" />
      </div>
      <div>
        <div className="font-mono text-2xl font-semibold tabular-nums text-zinc-100">
          {value}
        </div>
        <div className="mt-0.5 text-xs leading-tight text-zinc-500">{label}</div>
      </div>
    </Card>
  );
}
