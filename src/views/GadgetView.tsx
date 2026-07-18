import { useCallback, useEffect, useRef, useState } from "react";
import { Mic } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  getCompactMode,
  getRecordingElapsed,
  getRecordingState,
  setGadgetHitRect,
} from "../lib/tauri";

/** Number of bars rendered in the live waveform. */
const BAR_COUNT = 16;
const WAVE_BAR_WEIGHTS = [
  0.22, 0.34, 0.48, 0.62, 0.78, 0.92, 1, 0.88,
  0.82, 0.96, 0.74, 0.58, 0.46, 0.34, 0.26, 0.2,
];

/** Formats an elapsed millisecond count as `MM:SS`. */
function formatClock(ms: number): string {
  const totalSec = Math.floor(ms / 1000);
  const m = Math.floor(totalSec / 60);
  const s = totalSec % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

/**
 * Compact floating gadget rendered in the `"gadget"` window. Draggable by
 * pressing the pill (see `handleMouseDown` â†’ `startDragging`).
 *
 * The overlay window is a fixed, mostly-transparent box. To stop its
 * transparent area from swallowing clicks meant for whatever sits behind/around
 * it, the visible pill's rectangle is continuously reported to the backend
 * (`setGadgetHitRect`); a backend watcher then keeps the window click-through
 * everywhere except over the pill.
 *
 *  - Idle + compact   â†’ coral mic dot only.
 *  - Idle + normal    â†’ dot + "Haumea Voice" label.
 *  - Recording        â†’ pulsing dot, mini waveform, timer.
 */
export function GadgetApp() {
  const [recording, setRecording] = useState(false);
  const [compact, setCompact] = useState(false);
  const [transcribing, setTranscribing] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [audioLevel, setAudioLevel] = useState(0);
  const tickRef = useRef<number | null>(null);
  const pillRef = useRef<HTMLDivElement>(null);
  // Skip identical hit-rect IPCs (ResizeObserver + stateKey can re-fire with
  // the same box). Cuts unnecessary main-thread `set_gadget_hit_rect` traffic.
  const lastHitRectRef = useRef<{
    x: number;
    y: number;
    width: number;
    height: number;
  } | null>(null);

  // Reports the visible pill's rectangle (logical px, relative to the window's
  // top-left) to the backend so the overlay can be click-through outside it.
  // The pill is always centered in the full-window container, so we derive the
  // rect from the window size and the pill's *layout* box (offsetWidth/Height,
  // which ignore the entrance scale animation — keeping the rect stable).
  const reportHitRect = useCallback(() => {
    const el = pillRef.current;
    if (!el) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    if (!w || !h) return;
    const PAD = 4; // small forgiving margin around the pill
    const next = {
      x: (window.innerWidth - w) / 2 - PAD,
      y: (window.innerHeight - h) / 2 - PAD,
      width: w + PAD * 2,
      height: h + PAD * 2,
    };
    const prev = lastHitRectRef.current;
    if (
      prev &&
      Math.abs(prev.x - next.x) < 0.5 &&
      Math.abs(prev.y - next.y) < 0.5 &&
      Math.abs(prev.width - next.width) < 0.5 &&
      Math.abs(prev.height - next.height) < 0.5
    ) {
      return;
    }
    lastHitRectRef.current = next;
    setGadgetHitRect(next).catch(() => {
      // Outside Tauri (plain browser dev) or window gone — nothing to do.
    });
  }, []);

  useEffect(() => {
    let mounted = true;

    (async () => {
      try {
        const [rec, cmp, ms] = await Promise.all([
          getRecordingState(),
          getCompactMode(),
          getRecordingElapsed(),
        ]);
        if (mounted) {
          setRecording(rec);
          setCompact(cmp);
          if (rec && ms > 0) setElapsed(ms);
        }
      } catch {
        // Outside Tauri (plain browser dev).
      }
    })();

    const unlisteners: Array<Promise<() => void>> = [
      listen("recording-started", () => {
        setRecording(true);
        setElapsed(0);
        setAudioLevel(0);
      }),
      listen("recording-stopped", () => {
        setRecording(false);
        setAudioLevel(0);
      }),
      listen("recording-cancelled", () => {
        setRecording(false);
        setAudioLevel(0);
      }),
      listen<number>("audio-level", (e) => {
        const level = Number.isFinite(e.payload) ? e.payload : 0;
        const clamped = Math.max(0, Math.min(1, level));
        // Epsilon: skip React re-renders for imperceptible level noise.
        // Waveform still smooths via rAF + ref inside RecordingPill.
        setAudioLevel((prev) =>
          Math.abs(prev - clamped) < 0.008 ? prev : clamped,
        );
      }),
      listen<boolean>("transcribing", (e) => {
        setTranscribing(!!e.payload);
      }),
      listen<boolean>("compact-mode-changed", (e) => setCompact(!!e.payload)),
    ];

    return () => {
      mounted = false;
      unlisteners.forEach((p) => p.then((u) => u()));
    };
  }, []);

  useEffect(() => {
    if (recording) {
      tickRef.current = window.setInterval(
        () => setElapsed((e) => e + 200),
        200,
      );
    } else if (tickRef.current !== null) {
      window.clearInterval(tickRef.current);
      tickRef.current = null;
    }
    // Unconditionally clear the interval on cleanup. The previous guard
    // (`&& !recording`) leaked the timer if the gadget unmounted/remounted
    // (e.g. via React.StrictMode) while a recording was in progress. The
    // orphaned 200ms interval then kept calling setState on a dead React
    // root, eventually saturating the JS task queue and making the whole
    // overlay window appear hung ("NÃ£o respondendo") â€” the root cause of
    // the app freezing when focus moved away to another window.
    return () => {
      if (tickRef.current !== null) {
        window.clearInterval(tickRef.current);
        tickRef.current = null;
      }
    };
  }, [recording]);

  const handleMouseDown = (e: React.MouseEvent) => {
    if (e.button === 0) { // Clique esquerdo
      try {
        getCurrentWindow().startDragging();
      } catch (err) {
        console.error("Erro ao arrastar janela:", err);
      }
    }
  };

  const stateKey = transcribing
    ? "loading"
    : recording
      ? "rec"
      : compact
        ? "idle-compact"
        : "idle-full";

  // Re-report the pill rect whenever it remounts (state change) or resizes
  // (waveform/timer/label width changes), so the click-through region always
  // tracks the visible pill.
  useEffect(() => {
    reportHitRect();
    const el = pillRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => reportHitRect());
    ro.observe(el);
    return () => ro.disconnect();
  }, [stateKey, reportHitRect]);

  // Window size changes (e.g. DPI move) shift the centered pill too.
  useEffect(() => {
    const onResize = () => reportHitRect();
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [reportHitRect]);

  return (
    <div
      className="flex h-screen w-screen select-none items-center justify-center overflow-hidden bg-transparent"
    >
      <div
        key={stateKey}
        ref={pillRef}
        onMouseDown={handleMouseDown}
        className="animate-gadget-pop will-change-transform cursor-grab active:cursor-grabbing"
      >
        {transcribing ? (
          <TranscribingPill />
        ) : recording ? (
          <RecordingPill elapsed={elapsed} level={audioLevel} />
        ) : compact ? (
          <CompactOrb />
        ) : (
          <FullPill />
        )}
      </div>
    </div>
  );
}

/* ------------------------------- Idle states ------------------------------- */

function Orb({ size = "h-7 w-7" }: { size?: string }) {
  return (
    <div className="relative flex items-center justify-center pointer-events-none">
      <span className="absolute inline-flex h-full w-full rounded-full bg-coral-500/40 animate-ring-pulse pointer-events-none" />
      <div
        className={
          "relative flex items-center justify-center rounded-full bg-gradient-to-br from-coral-400 to-coral-600 shadow-glow-coral animate-breathe pointer-events-none " +
          size
        }
      >
        <Mic className="h-3.5 w-3.5 text-white pointer-events-none" strokeWidth={2.2} />
      </div>
    </div>
  );
}

function CompactOrb() {
  return (
    <div
      className="rounded-full border border-white/10 bg-zinc-900/80 p-1 shadow-gadget backdrop-blur-xl"
    >
      <Orb />
    </div>
  );
}

function FullPill() {
  return (
    <div
      className="flex items-center gap-2 rounded-full border border-white/10 bg-zinc-900/80 py-1 pl-1 pr-3.5 shadow-gadget backdrop-blur-xl"
    >
      <Orb />
      <div className="leading-none pointer-events-none">
        <div className="text-[11px] font-semibold tracking-tight text-zinc-100 pointer-events-none">
          Haumea Voice
        </div>
        <div className="text-[8px] font-medium uppercase tracking-wider text-zinc-500 pointer-events-none">
          Pronto
        </div>
      </div>
    </div>
  );
}

/* ------------------------------ Recording state ---------------------------- */

function RecordingPill({
  elapsed,
  level,
}: {
  elapsed: number;
  level: number;
}) {
  return (
    <div
      className="flex items-center gap-2.5 rounded-full border border-coral-500/30 bg-zinc-900/85 py-1.5 pl-3 pr-3.5 shadow-gadget backdrop-blur-xl"
    >
      {/* Pulsing live dot */}
      <span className="relative flex h-2 w-2 shrink-0 pointer-events-none">
        <span className="absolute inline-flex h-full w-full rounded-full bg-coral-500/60 animate-ring-pulse pointer-events-none" />
        <span className="relative inline-flex h-2 w-2 rounded-full bg-coral-500 animate-soft-pulse pointer-events-none" />
      </span>

      {/* Live waveform */}
      <div className="pointer-events-none flex items-center">
        <Waveform level={level} />
      </div>

      {/* Elapsed time */}
      <div className="shrink-0 font-mono text-[11px] font-medium tabular-nums text-zinc-100 pointer-events-none">
        {formatClock(elapsed)}
      </div>
    </div>
  );
}

function Waveform({ level }: { level: number }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const targetLevelRef = useRef(0);
  const smoothLevelRef = useRef(0);

  useEffect(() => {
    // Extra frontend boost so quiet speech still moves the bars hard.
    targetLevelRef.current = Math.min(1, level * 1.35);
  }, [level]);

  useEffect(() => {
    let animFrame: number;
    let time = 0;

    const tick = () => {
      time += 0.16;

      if (containerRef.current) {
        const spans = containerRef.current.children;
        // Fast attack / medium release so speech pops immediately.
        const target = targetLevelRef.current;
        const current = smoothLevelRef.current;
        const alpha = target > current ? 0.42 : 0.22;
        smoothLevelRef.current += (target - current) * alpha;
        const voice = smoothLevelRef.current < 0.008 ? 0 : smoothLevelRef.current;

        for (let i = 0; i < spans.length; i++) {
          const weight = WAVE_BAR_WEIGHTS[i] ?? 0.3;
          const shimmer = voice > 0
            ? 1 + Math.sin(time + i * 0.72) * 0.14 + Math.sin(time * 0.9 + i) * 0.1
            : 1;
          // Taller bars + more of the available height while speaking.
          const height = 12 + voice * 92 * weight * shimmer;
          const opacity = 0.55 + voice * 0.45 * Math.max(0.55, weight);
          const bar = spans[i] as HTMLElement;
          bar.style.height = `${Math.max(10, Math.min(100, height))}%`;
          bar.style.opacity = `${Math.max(0.45, Math.min(1, opacity))}`;
          // Glow intensity scales with voice so loud speech lights up coral/amber.
          const glow = 0.35 + voice * 0.75;
          bar.style.boxShadow = `0 0 ${6 + voice * 14}px rgba(255, 120, 60, ${glow}), 0 0 ${4 + voice * 8}px rgba(255, 200, 80, ${glow * 0.55})`;
        }
      }
      animFrame = requestAnimationFrame(tick);
    };

    animFrame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(animFrame);
  }, []);

  return (
    <div ref={containerRef} className="flex h-7 w-[58px] items-center justify-center gap-[2.5px] pointer-events-none">
      {Array.from({ length: BAR_COUNT }).map((_, i) => (
        <span
          key={i}
          className="w-[3px] rounded-full bg-gradient-to-t from-coral-600 via-coral-400 to-amber-100 pointer-events-none transition-none will-change-[height,opacity,box-shadow]"
          style={{
            height: "12%",
            opacity: 0.5,
            boxShadow: "0 0 6px rgba(255, 120, 60, 0.4)",
          }}
        />
      ))}
    </div>
  );
}

function TranscribingPill() {
  return (
    <div
      className="flex items-center gap-2.5 rounded-full border border-coral-500/30 bg-zinc-900/85 py-1.5 pl-3.5 pr-4 shadow-gadget backdrop-blur-xl animate-pulse"
    >
      <div className="flex items-center gap-1 shrink-0 pointer-events-none">
        <span className="h-1.5 w-1.5 rounded-full bg-coral-500 animate-bounce [animation-delay:-0.3s] pointer-events-none" />
        <span className="h-1.5 w-1.5 rounded-full bg-coral-500 animate-bounce [animation-delay:-0.15s] pointer-events-none" />
        <span className="h-1.5 w-1.5 rounded-full bg-coral-500 animate-bounce pointer-events-none" />
      </div>
      <div className="text-[11px] font-semibold tracking-tight text-zinc-100 pointer-events-none">
        Processando...
      </div>
    </div>
  );
}
