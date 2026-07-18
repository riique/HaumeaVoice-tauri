import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Plus,
  X,
  Zap,
  Rocket,
  Target,
  Gem,
  KeyRound,
  Activity,
  Mic,
  Eye,
  EyeOff,
  Save,
  AlertCircle,
  CheckCircle2,
  FlaskConical,
  type LucideIcon,
} from "lucide-react";
import {
  getCompactMode,
  setCompactMode,
  listAudioDevices,
  getInputDevice,
  setInputDevice,
  startMicTest,
  stopMicTest,
  onMicTestLevel,
  getEngineConfig,
  getVocabulary,
  setVocabulary,
  getDevMode,
  setDevMode,
  getSanitizerEnabled,
  setSanitizerEnabled,
  getModeConfig,
  updateModeConfig,
  getApiKeys,
  saveApiKeys,
  type DeepgramMode,
  type SanitizerModel,
  type TranscriptionMode,
  type ContentType,
  type VocabularyTerm,
  type VocabularyCategory,
} from "../lib/tauri";
import { Button } from "../components/ui/Button";
import { Card } from "../components/ui/Card";
import { Input } from "../components/ui/Input";
import { Toggle } from "../components/ui/Toggle";

type Tab =
  | "geral"
  | "pipelines"
  | "provedores"
  | "vocabulario"
  | "diagnostico";

const TABS: { key: Tab; label: string }[] = [
  { key: "geral", label: "Geral" },
  { key: "pipelines", label: "Pipelines" },
  { key: "provedores", label: "Provedores e APIs" },
  { key: "vocabulario", label: "Vocabulário" },
  { key: "diagnostico", label: "Diagnóstico" },
];

const MODE_CARDS: {
  id: TranscriptionMode;
  title: string;
  engine: string;
  blurb: string;
  Icon: LucideIcon;
}[] = [
  {
    id: "ultra-fast",
    title: "Ultrarrápido",
    engine: "Whisper",
    blurb: "Menor latência",
    Icon: Zap,
  },
  {
    id: "fast-accurate",
    title: "Rápido e preciso",
    engine: "Gemini com áudio",
    blurb: "Boa precisão com menos etapas",
    Icon: Rocket,
  },
  {
    id: "precise",
    title: "Preciso",
    engine: "Whisper + Gemini",
    blurb: "Melhor equilíbrio geral",
    Icon: Target,
  },
  {
    id: "ultra-precise",
    title: "Ultrapreciso",
    engine: "Pipeline completa",
    blurb: "Para conteúdo importante",
    Icon: Gem,
  },
];

const CONTENT_TYPES: { id: ContentType; label: string; hint: string }[] = [
  {
    id: "auto",
    label: "Automático",
    hint: "Detecta pelo texto (código, estudo ou fala comum)",
  },
  {
    id: "programming",
    label: "Programação",
    hint: "Preserva literais, comandos e caminhos",
  },
  {
    id: "general-speech",
    label: "Texto comum",
    hint: "Fluidez conservadora no dia a dia",
  },
  {
    id: "study",
    label: "Estudo",
    hint: "Terminologia e estrutura explicativa",
  },
];

export function ConfiguracoesView() {
  const [tab, setTab] = useState<Tab>("pipelines");

  return (
    <div className="space-y-8">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight text-zinc-100">
          Configurações
        </h1>
        <p className="mt-1 text-sm text-zinc-500 max-w-2xl">
          Escolha como o Haumea Voice transcreve, conecte suas chaves e ajuste o
          vocabulário. Um pipeline ativo por vez — sem promessas de perfeição
          absoluta.
        </p>
      </header>

      <div
        className="flex gap-1 border-b border-zinc-800/60 overflow-x-auto"
        role="tablist"
        aria-label="Seções de configuração"
      >
        {TABS.map((t) => (
          <button
            key={t.key}
            role="tab"
            aria-selected={tab === t.key}
            onClick={() => setTab(t.key)}
            className={
              "relative shrink-0 px-4 py-3 text-sm font-medium transition-colors duration-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-coral-500/40 rounded-t-lg " +
              (tab === t.key
                ? "text-coral-400"
                : "text-zinc-500 hover:text-zinc-300")
            }
          >
            {t.label}
            {tab === t.key && (
              <span className="absolute bottom-0 left-0 right-0 h-0.5 bg-coral-500" />
            )}
          </button>
        ))}
      </div>

      {tab === "geral" && <GeralTab />}
      {tab === "pipelines" && <PipelinesTab />}
      {tab === "provedores" && <ProvedoresTab />}
      {tab === "vocabulario" && <VocabularioTab />}
      {tab === "diagnostico" && <DiagnosticoTab />}
    </div>
  );
}

/* --------------------------------- Geral --------------------------------- */

function GeralTab() {
  const [startup, setStartup] = useState(false);
  const [compact, setCompact] = useState(false);
  const [devices, setDevices] = useState<string[]>([]);
  const [selectedDevice, setSelectedDevice] = useState<string | null>(null);
  const [isTesting, setIsTesting] = useState(false);
  const [micLevel, setMicLevel] = useState(0);

  useEffect(() => {
    invoke<boolean>("get_autostart")
      .then(setStartup)
      .catch((e) => console.error("get_autostart failed:", e));
    getCompactMode()
      .then(setCompact)
      .catch((e) => console.error("get_compact_mode failed:", e));
    listAudioDevices()
      .then(setDevices)
      .catch((e) => console.error("listAudioDevices failed:", e));
    getInputDevice()
      .then(setSelectedDevice)
      .catch((e) => console.error("getInputDevice failed:", e));
  }, []);

  useEffect(() => {
    if (!isTesting) {
      setMicLevel(0);
      return;
    }
    let active = true;
    let unlisten: (() => void) | null = null;
    onMicTestLevel((level) => {
      if (active) setMicLevel(level);
    })
      .then((unsub) => {
        if (active) unlisten = unsub;
        else unsub();
      })
      .catch(() => {});
    return () => {
      active = false;
      unlisten?.();
    };
  }, [isTesting]);

  useEffect(() => {
    return () => {
      stopMicTest().catch(() => {});
    };
  }, []);

  return (
    <div className="space-y-6">
      <Card className="divide-y divide-zinc-800/60 p-2">
        <Row
          title="Iniciar com o Windows"
          description="Abre o Haumea Voice ao ligar o computador (em segundo plano se usar autostart)."
        >
          <Toggle
            checked={startup}
            onChange={(v) => {
              setStartup(v);
              invoke("set_autostart", { enabled: v }).catch(console.error);
            }}
          />
        </Row>
        <Row
          title="Gadget compacto"
          description="Com ocioso, o overlay mostra só o ícone. Ao gravar, expande automaticamente."
        >
          <Toggle
            checked={compact}
            onChange={(v) => {
              setCompact(v);
              setCompactMode(v).catch(console.error);
            }}
          />
        </Row>
        <Row
          title="Bandeja do sistema"
          description="Ao fechar a janela, o app continua na bandeja. Use Sair no menu do ícone para encerrar de verdade."
        >
          <span className="text-xs font-medium text-emerald-400/90 shrink-0">
            Sempre ativo
          </span>
        </Row>
      </Card>

      <Card className="p-7 space-y-5">
        <div className="flex items-center gap-2.5">
          <Mic className="h-5 w-5 text-coral-400" />
          <h3 className="text-sm font-semibold text-zinc-100">
            Microfone de entrada
          </h3>
        </div>
        <p className="text-xs text-zinc-500 leading-relaxed">
          Escolha o microfone das gravações. Se o dispositivo sumir, o app usa o
          padrão do sistema.
        </p>
        <div className="flex flex-col sm:flex-row gap-4 items-stretch sm:items-center">
          <select
            value={selectedDevice || "default"}
            onChange={async (e) => {
              const val = e.target.value === "default" ? null : e.target.value;
              setSelectedDevice(val);
              await setInputDevice(val);
              setDevices(await listAudioDevices());
            }}
            className="w-full max-w-md bg-zinc-900 border border-zinc-800/80 text-zinc-200 text-xs rounded-xl px-4 py-3 outline-none focus:border-coral-500/40 focus-visible:ring-2 focus-visible:ring-coral-500/30"
          >
            <option value="default">Padrão do sistema</option>
            {devices.map((d) => (
              <option key={d} value={d}>
                {d}
              </option>
            ))}
          </select>
          <Button
            variant={isTesting ? "secondary" : "primary"}
            onClick={async () => {
              if (isTesting) {
                await stopMicTest();
                setIsTesting(false);
              } else {
                await startMicTest();
                setIsTesting(true);
              }
            }}
            className="h-[42px] px-6 text-xs gap-2"
          >
            <span
              className={
                "h-2 w-2 rounded-full " +
                (isTesting ? "bg-red-500 animate-pulse" : "bg-zinc-500")
              }
            />
            {isTesting ? "Parar teste" : "Testar microfone"}
          </Button>
        </div>
        {isTesting && (
          <div className="space-y-2" aria-live="polite">
            <div className="flex justify-between text-[10px] uppercase tracking-wider text-zinc-500">
              <span>Nível de entrada</span>
              <span className="font-mono">{Math.round(micLevel * 100)}%</span>
            </div>
            <div className="h-2 w-full bg-zinc-900/60 rounded-full overflow-hidden border border-zinc-800/40">
              <div
                className="h-full bg-gradient-to-r from-coral-500 via-coral-400 to-amber-400 rounded-full transition-all duration-75"
                style={{ width: `${micLevel * 100}%` }}
              />
            </div>
          </div>
        )}
      </Card>
    </div>
  );
}

/* ------------------------------- Pipelines ------------------------------- */

function PipelinesTab() {
  const [modesEnabled, setModesEnabled] = useState(true);
  const [mode, setMode] = useState<TranscriptionMode>("ultra-fast");
  const [contentType, setContentType] = useState<ContentType>("auto");
  const [geminiFallback, setGeminiFallback] = useState(true);
  const [sanitizer, setSanitizer] = useState<SanitizerModel>("llama-70b");
  const [sanitizerEnabled, setSanitizerEnabledState] = useState(true);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [engine, setEngine] = useState("groq-whisper");
  const [dual, setDual] = useState(false);
  const [deepgramMode, setDeepgramMode] = useState<DeepgramMode>("batch");
  const [reasoning, setReasoning] = useState(false);
  const [effort, setEffort] = useState("medium");
  const [status, setStatus] = useState("");

  const persistMode = (p: {
    modes_enabled?: boolean;
    mode?: TranscriptionMode;
    gemini_fallback_to_whisper?: boolean;
    content_type?: ContentType;
  }) => {
    const payload = {
      modes_enabled: p.modes_enabled ?? modesEnabled,
      mode: p.mode ?? mode,
      gemini_fallback_to_whisper: p.gemini_fallback_to_whisper ?? geminiFallback,
      content_type: p.content_type ?? contentType,
    };
    updateModeConfig(payload)
      .then(() => setStatus("Pipeline salva"))
      .catch((e) => setStatus(String(e)));
  };

  useEffect(() => {
    getModeConfig()
      .then((m) => {
        setModesEnabled(m.modes_enabled);
        setMode(m.mode);
        setGeminiFallback(m.gemini_fallback_to_whisper);
        setContentType(m.content_type || "auto");
      })
      .catch(console.error);
    getEngineConfig()
      .then((c) => {
        if (c.engine) setEngine(c.engine);
        setDual(!!c.dual_engine);
        if (c.sanitizer) setSanitizer(c.sanitizer as SanitizerModel);
        setReasoning(!!c.reasoning_enabled);
        if (c.reasoning_effort) setEffort(c.reasoning_effort);
        if (c.deepgram_mode) setDeepgramMode(c.deepgram_mode);
      })
      .catch(console.error);
    getSanitizerEnabled().then(setSanitizerEnabledState).catch(console.error);
  }, []);

  const selected = MODE_CARDS.find((c) => c.id === mode)!;
  const SelectedIcon = selected.Icon;

  const selectMode = (id: TranscriptionMode) => {
    setMode(id);
    setModesEnabled(true);
    persistMode({ mode: id, modes_enabled: true });
  };

  return (
    <div className="space-y-6">
      <Card className="p-6 border-coral-500/30 bg-coral-500/5">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <p className="text-[11px] font-medium uppercase tracking-wider text-coral-400/80">
              Pipeline ativa
            </p>
            <h2 className="mt-1 flex items-center gap-2.5 text-lg font-semibold text-zinc-100">
              <span className="flex h-8 w-8 items-center justify-center rounded-lg bg-coral-500/20 text-coral-400">
                <SelectedIcon className="h-4 w-4" aria-hidden />
              </span>
              {selected.title}
            </h2>
            <p className="mt-1 text-sm text-zinc-400">
              {selected.engine} · {selected.blurb}
            </p>
            {!modesEnabled && (
              <p className="mt-2 text-xs text-amber-400/90 flex items-center gap-1.5">
                <AlertCircle className="h-3.5 w-3.5" />
                Modos desligados — o fluxo legado (motores manuais) está em uso.
              </p>
            )}
          </div>
          {status && (
            <span className="inline-flex items-center gap-1.5 text-xs text-emerald-400">
              <CheckCircle2 className="h-3.5 w-3.5" />
              {status}
            </span>
          )}
        </div>
      </Card>

      <div>
        <h3 className="text-sm font-semibold text-zinc-100 mb-3">
          Escolha um modo
        </h3>
        <div
          className="grid grid-cols-1 sm:grid-cols-2 gap-3"
          role="radiogroup"
          aria-label="Modo de transcrição"
        >
          {MODE_CARDS.map((card) => {
            const active = modesEnabled && mode === card.id;
            const Icon = card.Icon;
            return (
              <button
                key={card.id}
                type="button"
                role="radio"
                aria-checked={active}
                onClick={() => selectMode(card.id)}
                className={
                  "text-left rounded-2xl border p-5 transition-all duration-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-coral-500/40 " +
                  (active
                    ? "border-coral-500/60 bg-coral-500/10 shadow-[0_0_24px_-10px_rgba(225,77,42,0.45)]"
                    : "border-zinc-800/70 bg-zinc-900/40 hover:border-zinc-700")
                }
              >
                <div className="flex items-start justify-between gap-3">
                  <div
                    className={
                      "flex h-10 w-10 items-center justify-center rounded-xl " +
                      (active
                        ? "bg-coral-500/20 text-coral-400"
                        : "bg-zinc-800 text-zinc-400")
                    }
                  >
                    <Icon className="h-5 w-5" aria-hidden />
                  </div>
                  {active && (
                    <span className="text-[10px] font-medium uppercase tracking-wider text-coral-400">
                      Selecionado
                    </span>
                  )}
                </div>
                <div className="mt-4 text-base font-semibold text-zinc-100">
                  {card.title}
                </div>
                <div className="mt-1 text-xs font-medium text-coral-400/80">
                  {card.engine}
                </div>
                <p className="mt-2 text-sm text-zinc-500 leading-relaxed">
                  {card.blurb}
                </p>
              </button>
            );
          })}
        </div>
      </div>

      <Card className="p-6 space-y-4">
        <h3 className="text-sm font-semibold text-zinc-100">Tipo de conteúdo</h3>
        <p className="text-xs text-zinc-500">
          Ajusta o tom do validador e do Gemini. Não garante resultado perfeito —
          só orienta o pipeline.
        </p>
        <div
          className="grid grid-cols-1 sm:grid-cols-2 gap-2"
          role="radiogroup"
          aria-label="Tipo de conteúdo"
        >
          {CONTENT_TYPES.map((ct) => {
            const on = contentType === ct.id;
            return (
              <button
                key={ct.id}
                type="button"
                role="radio"
                aria-checked={on}
                onClick={() => {
                  setContentType(ct.id);
                  persistMode({ content_type: ct.id });
                }}
                className={
                  "rounded-xl border px-4 py-3 text-left transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-coral-500/40 " +
                  (on
                    ? "border-coral-500/50 bg-coral-500/10"
                    : "border-zinc-800 bg-zinc-900/30 hover:border-zinc-700")
                }
              >
                <div className="text-sm font-medium text-zinc-100">{ct.label}</div>
                <div className="mt-0.5 text-xs text-zinc-500">{ct.hint}</div>
              </button>
            );
          })}
        </div>
      </Card>

      {mode === "fast-accurate" && modesEnabled && (
        <Card className="p-5 flex items-center justify-between gap-4">
          <div>
            <h4 className="text-sm font-medium text-zinc-100">
              Se o Gemini falhar, usar Whisper
            </h4>
            <p className="text-xs text-zinc-500 mt-1">
              O histórico marca quando o fallback acontecer.
            </p>
          </div>
          <Toggle
            checked={geminiFallback}
            onChange={(v) => {
              setGeminiFallback(v);
              persistMode({ gemini_fallback_to_whisper: v });
            }}
          />
        </Card>
      )}

      {(mode === "ultra-precise" || !modesEnabled) && (
        <Card className="p-5 space-y-3">
          <div className="flex items-center justify-between">
            <div>
              <h4 className="text-sm font-medium text-zinc-100">
                Validador semântico
              </h4>
              <p className="text-xs text-zinc-500 mt-1">
                Limpa ortografia após o Whisper (Ultrapreciso e fluxo legado).
              </p>
            </div>
            <Toggle
              checked={sanitizerEnabled}
              onChange={(v) => {
                setSanitizerEnabledState(v);
                setSanitizerEnabled(v).catch(console.error);
              }}
            />
          </div>
          {sanitizerEnabled && (
            <div className="grid grid-cols-2 gap-2 pt-2">
              {(
                [
                  ["llama-70b", "LLaMA 70B"],
                  ["gpt-oss-20b", "GPT-OSS 20B"],
                  ["gpt-oss-120b", "GPT-OSS 120B"],
                  ["qwen3-27b", "Qwen 3.6 27B"],
                ] as const
              ).map(([id, label]) => (
                <button
                  key={id}
                  type="button"
                  onClick={() => {
                    setSanitizer(id);
                    invoke("update_engine_config", {
                      payload: {
                        engine,
                        sanitizer: id,
                        dual_engine: dual,
                        reasoning_enabled: reasoning,
                        reasoning_effort: effort,
                        deepgram_mode: deepgramMode,
                      },
                    }).catch(console.error);
                  }}
                  className={
                    "rounded-xl border py-2 text-xs font-medium " +
                    (sanitizer === id
                      ? "border-coral-500/50 bg-coral-500/10 text-coral-400"
                      : "border-zinc-800 text-zinc-400 hover:bg-zinc-900")
                  }
                >
                  {label}
                </button>
              ))}
            </div>
          )}
        </Card>
      )}

      {/* Advanced / experimental */}
      <div className="pt-2">
        <button
          type="button"
          onClick={() => setShowAdvanced((v) => !v)}
          className="text-xs font-medium text-zinc-500 hover:text-zinc-300 flex items-center gap-2"
        >
          <FlaskConical className="h-3.5 w-3.5" />
          {showAdvanced ? "Ocultar avançado" : "Avançado e experimental"}
        </button>
      </div>

      {showAdvanced && (
        <Card className="p-6 space-y-5 border-zinc-800/80">
          <div className="flex items-start gap-2">
            <AlertCircle className="h-4 w-4 text-amber-400 shrink-0 mt-0.5" />
            <p className="text-xs text-zinc-500 leading-relaxed">
              Opções legadas. Com um modo de pipeline ativo, o dual engine e a
              escolha manual de motor{" "}
              <strong className="text-zinc-400">não</strong> definem o caminho
              principal — só valem se você desligar os modos abaixo.
            </p>
          </div>

          <div className="flex items-center justify-between gap-4">
            <div>
              <h4 className="text-sm font-medium text-zinc-100">
                Usar modos de pipeline
              </h4>
              <p className="text-xs text-zinc-500">
                Desligado = fluxo antigo (motor + dual + Deepgram).
              </p>
            </div>
            <Toggle
              checked={modesEnabled}
              onChange={(v) => {
                setModesEnabled(v);
                persistMode({ modes_enabled: v });
              }}
            />
          </div>

          <div className={"space-y-4 " + (modesEnabled ? "opacity-40" : "")}>
            <div className="flex items-center justify-between gap-4">
              <div>
                <h4 className="text-sm font-medium text-zinc-100">
                  Dual Whisper + Deepgram
                </h4>
                <p className="text-xs text-zinc-500">
                  Só no fluxo legado. Não se mistura com os cards de modo.
                </p>
              </div>
              <Toggle
                checked={dual}
                disabled={modesEnabled}
                onChange={(v) => {
                  setDual(v);
                  invoke("update_engine_config", {
                    payload: {
                      engine,
                      sanitizer,
                      dual_engine: v,
                      reasoning_enabled: reasoning,
                      reasoning_effort: effort,
                      deepgram_mode: deepgramMode,
                    },
                  }).catch(console.error);
                }}
              />
            </div>

            <div className="space-y-2">
              <label className="text-[11px] font-medium uppercase tracking-wider text-zinc-500">
                Deepgram (experimental)
              </label>
              <div className="flex gap-2">
                {(
                  [
                    ["batch", "Batch"],
                    ["streaming_final", "Streaming final"],
                  ] as const
                ).map(([id, label]) => (
                  <button
                    key={id}
                    type="button"
                    disabled={modesEnabled}
                    onClick={() => {
                      setDeepgramMode(id);
                      invoke("update_engine_config", {
                        payload: {
                          engine: dual ? engine : "deepgram-nova3",
                          sanitizer,
                          dual_engine: dual,
                          reasoning_enabled: reasoning,
                          reasoning_effort: effort,
                          deepgram_mode: id,
                        },
                      }).catch(console.error);
                    }}
                    className={
                      "flex-1 py-2 rounded-xl text-xs font-medium border " +
                      (deepgramMode === id
                        ? "bg-coral-500 text-white border-coral-500"
                        : "bg-zinc-900 border-zinc-800 text-zinc-400")
                    }
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>

            <div className="flex items-center justify-between gap-4">
              <div>
                <h4 className="text-sm font-medium text-zinc-100">
                  Reasoning no validador (GPT-OSS)
                </h4>
                <p className="text-xs text-zinc-500">
                  Desligado por padrão. Só modelos GPT-OSS.
                </p>
              </div>
              <Toggle
                checked={reasoning}
                onChange={(v) => {
                  setReasoning(v);
                  invoke("update_engine_config", {
                    payload: {
                      engine,
                      sanitizer,
                      dual_engine: dual,
                      reasoning_enabled: v,
                      reasoning_effort: effort,
                      deepgram_mode: deepgramMode,
                    },
                  }).catch(console.error);
                }}
              />
            </div>
          </div>
        </Card>
      )}
    </div>
  );
}

/* --------------------------- Provedores e APIs --------------------------- */

type KeyId = "groq" | "google" | "deepgram";

function ProvedoresTab() {
  const [keys, setKeys] = useState({ groq: "", google: "", deepgram: "" });
  const [visible, setVisible] = useState<Record<KeyId, boolean>>({
    groq: false,
    google: false,
    deepgram: false,
  });
  const [saving, setSaving] = useState<KeyId | null>(null);
  const [saved, setSaved] = useState<KeyId | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    getApiKeys()
      .then((k) =>
        setKeys({
          groq: k.groq ?? "",
          google: k.google ?? "",
          deepgram: k.deepgram ?? "",
        }),
      )
      .catch(console.error);
  }, []);

  const providers: {
    id: KeyId;
    name: string;
    placeholder: string;
    help: string;
    requiredFor: string;
  }[] = [
    {
      id: "groq",
      name: "Groq",
      placeholder: "gsk_…",
      help: "Whisper, validador e fallbacks.",
      requiredFor: "Ultrarrápido, Preciso, Ultrapreciso",
    },
    {
      id: "google",
      name: "Google (Gemini)",
      placeholder: "AIza…",
      help: "Transcrição Gemini e avaliação de pronúncia no Histórico.",
      requiredFor: "Rápido e preciso, Preciso, Ultrapreciso, Pronúncia",
    },
    {
      id: "deepgram",
      name: "Deepgram",
      placeholder: "uuid…",
      help: "Apenas no fluxo legado / experimental.",
      requiredFor: "Avançado · dual / Deepgram",
    },
  ];

  const statusFor = (id: KeyId) => {
    const v = keys[id].trim();
    if (!v) return { label: "Sem chave", tone: "text-zinc-500" };
    return { label: "Configurada", tone: "text-emerald-400" };
  };

  const save = async (id: KeyId) => {
    setSaving(id);
    setError("");
    try {
      await saveApiKeys({
        groq: keys.groq,
        google: keys.google,
        deepgram: keys.deepgram,
      });
      setSaved(id);
      window.setTimeout(() => setSaved((c) => (c === id ? null : c)), 2000);
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setSaving(null);
    }
  };

  return (
    <div className="space-y-6">
      <p className="text-sm text-zinc-500">
        As chaves ficam só neste computador (AppData). Nunca são enviadas a
        servidores do Haumea.
      </p>

      {error && (
        <div
          className="flex gap-2 rounded-xl border border-red-900/40 bg-red-950/20 px-4 py-3 text-sm text-red-300"
          role="alert"
        >
          <AlertCircle className="h-4 w-4 shrink-0 mt-0.5" />
          {error}
        </div>
      )}

      <div className="grid gap-4">
        {providers.map((p) => {
          const st = statusFor(p.id);
          return (
            <Card key={p.id} className="p-6 space-y-4">
              <div className="flex items-start justify-between gap-3">
                <div className="flex items-center gap-2.5">
                  <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-zinc-800 text-coral-400">
                    <KeyRound className="h-4 w-4" />
                  </div>
                  <div>
                    <h3 className="text-sm font-semibold text-zinc-100">
                      {p.name}
                    </h3>
                    <p className="text-[11px] text-zinc-500">{p.requiredFor}</p>
                  </div>
                </div>
                <span className={"text-[11px] font-medium " + st.tone}>
                  {st.label}
                </span>
              </div>
              <p className="text-xs text-zinc-500">{p.help}</p>
              <div className="relative">
                <Input
                  type={visible[p.id] ? "text" : "password"}
                  placeholder={p.placeholder}
                  value={keys[p.id]}
                  onChange={(e) =>
                    setKeys((k) => ({ ...k, [p.id]: e.target.value }))
                  }
                  autoComplete="off"
                  spellCheck={false}
                  className="pr-10 text-xs font-mono"
                  aria-label={`Chave ${p.name}`}
                />
                <button
                  type="button"
                  className="absolute right-3 top-1/2 -translate-y-1/2 text-zinc-500 hover:text-zinc-300"
                  onClick={() =>
                    setVisible((v) => ({ ...v, [p.id]: !v[p.id] }))
                  }
                  aria-label={
                    visible[p.id] ? "Ocultar chave" : "Mostrar chave"
                  }
                >
                  {visible[p.id] ? (
                    <EyeOff className="h-4 w-4" />
                  ) : (
                    <Eye className="h-4 w-4" />
                  )}
                </button>
              </div>
              <Button
                variant="secondary"
                className="w-full gap-2 text-xs"
                disabled={saving === p.id}
                onClick={() => save(p.id)}
              >
                <Save className="h-3.5 w-3.5" />
                {saving === p.id
                  ? "Salvando…"
                  : saved === p.id
                    ? "Salvo"
                    : "Salvar chave"}
              </Button>
            </Card>
          );
        })}
      </div>
    </div>
  );
}

/* ------------------------------- Vocabulário ------------------------------ */

const VOCAB_CATEGORIES: { id: VocabularyCategory; label: string }[] = [
  { id: "ai_model", label: "Modelo de IA" },
  { id: "provider", label: "Provedor" },
  { id: "application", label: "Aplicativo" },
  { id: "person", label: "Pessoa" },
  { id: "file", label: "Arquivo" },
  { id: "command", label: "Comando" },
  { id: "function", label: "Função" },
  { id: "identifier", label: "Identificador" },
  { id: "study_term", label: "Termo de estudo" },
  { id: "other", label: "Outro" },
];

function emptyTerm(): VocabularyTerm {
  return {
    canonical: "",
    aliases: [],
    category: "other",
    strict: false,
    enabled: true,
  };
}

function VocabularioTab() {
  const [terms, setTerms] = useState<VocabularyTerm[]>([]);
  const [query, setQuery] = useState("");
  const [draft, setDraft] = useState<VocabularyTerm>(emptyTerm());
  const [aliasDraft, setAliasDraft] = useState("");
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    getVocabulary()
      .then(setTerms)
      .catch(console.error)
      .finally(() => setLoading(false));
  }, []);

  const persist = async (next: VocabularyTerm[]) => {
    setSaving(true);
    setError("");
    try {
      const cleaned = await setVocabulary(next);
      setTerms(cleaned);
      setDraft(emptyTerm());
      setAliasDraft("");
      setEditingIndex(null);
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setSaving(false);
    }
  };

  const filtered = query.trim()
    ? terms.filter((t) => {
        const q = query.trim().toLowerCase();
        return (
          t.canonical.toLowerCase().includes(q) ||
          t.aliases.some((a) => a.toLowerCase().includes(q)) ||
          t.category.includes(q)
        );
      })
    : terms;

  const categoryLabel = (id: VocabularyCategory) =>
    VOCAB_CATEGORIES.find((c) => c.id === id)?.label ?? id;

  return (
    <div className="space-y-6">
      <Card className="p-6 border-zinc-800/60">
        <h3 className="text-sm font-semibold text-zinc-100">
          Vocabulário estruturado
        </h3>
        <p className="mt-2 text-xs text-zinc-500 leading-relaxed">
          Cadastre a grafia correta e as variações da fala.{" "}
          <strong className="text-zinc-400">Literal</strong> protege arquivos,
          comandos e identificadores. A correção só age quando o encaixe é
          claro — sem substituição cega.
        </p>
      </Card>

      <Card className="p-6 space-y-4">
        <div className="flex items-center justify-between">
          <h4 className="text-sm font-medium text-zinc-100">
            {editingIndex !== null ? "Editar termo" : "Novo termo"}
          </h4>
          {editingIndex !== null && (
            <button
              type="button"
              className="text-xs text-zinc-500 hover:text-zinc-300"
              onClick={() => {
                setEditingIndex(null);
                setDraft(emptyTerm());
                setError("");
              }}
            >
              Cancelar
            </button>
          )}
        </div>
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <div className="space-y-1.5 sm:col-span-2">
            <label className="text-[11px] font-medium uppercase tracking-wider text-zinc-500">
              Grafia correta
            </label>
            <Input
              placeholder="ex: provider-routing.json"
              value={draft.canonical}
              onChange={(e) =>
                setDraft((d) => ({ ...d, canonical: e.target.value }))
              }
              autoComplete="off"
              spellCheck={false}
            />
          </div>
          <div className="space-y-1.5">
            <label className="text-[11px] font-medium uppercase tracking-wider text-zinc-500">
              Categoria
            </label>
            <select
              value={draft.category}
              onChange={(e) =>
                setDraft((d) => ({
                  ...d,
                  category: e.target.value as VocabularyCategory,
                }))
              }
              className="w-full bg-zinc-900 border border-zinc-800/80 text-zinc-200 text-xs rounded-xl px-4 py-3 outline-none focus:border-coral-500/40"
            >
              {VOCAB_CATEGORIES.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.label}
                </option>
              ))}
            </select>
          </div>
          <div className="flex items-end gap-4 pb-1">
            <label className="flex items-center gap-2 text-xs text-zinc-400 cursor-pointer">
              <input
                type="checkbox"
                checked={draft.strict}
                onChange={(e) =>
                  setDraft((d) => ({ ...d, strict: e.target.checked }))
                }
              />
              Literal
            </label>
            <label className="flex items-center gap-2 text-xs text-zinc-400 cursor-pointer">
              <input
                type="checkbox"
                checked={draft.enabled}
                onChange={(e) =>
                  setDraft((d) => ({ ...d, enabled: e.target.checked }))
                }
              />
              Ativo
            </label>
          </div>
        </div>
        <div className="space-y-1.5">
          <label className="text-[11px] font-medium uppercase tracking-wider text-zinc-500">
            Variações da fala
          </label>
          <div className="flex gap-2">
            <Input
              placeholder="ex: provider routing json"
              value={aliasDraft}
              onChange={(e) => setAliasDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  const a = aliasDraft.trim();
                  if (!a) return;
                  setDraft((d) => ({
                    ...d,
                    aliases: d.aliases.includes(a)
                      ? d.aliases
                      : [...d.aliases, a],
                  }));
                  setAliasDraft("");
                }
              }}
              className="flex-1"
              autoComplete="off"
              spellCheck={false}
            />
            <Button
              variant="secondary"
              type="button"
              onClick={() => {
                const a = aliasDraft.trim();
                if (!a) return;
                setDraft((d) => ({
                  ...d,
                  aliases: d.aliases.includes(a) ? d.aliases : [...d.aliases, a],
                }));
                setAliasDraft("");
              }}
            >
              Incluir
            </Button>
          </div>
          {draft.aliases.length > 0 && (
            <div className="flex flex-wrap gap-2 pt-1">
              {draft.aliases.map((a) => (
                <span
                  key={a}
                  className="inline-flex items-center gap-1.5 rounded-lg border border-zinc-800 bg-zinc-900 px-2.5 py-1 text-xs text-zinc-300"
                >
                  {a}
                  <button
                    type="button"
                    className="text-zinc-600 hover:text-red-400"
                    onClick={() =>
                      setDraft((d) => ({
                        ...d,
                        aliases: d.aliases.filter((x) => x !== a),
                      }))
                    }
                    aria-label={`Remover ${a}`}
                  >
                    <X className="h-3 w-3" />
                  </button>
                </span>
              ))}
            </div>
          )}
        </div>
        {error && (
          <p className="text-xs text-red-400" role="alert">
            {error}
          </p>
        )}
        <Button
          variant="primary"
          className="gap-2"
          disabled={saving || !draft.canonical.trim()}
          onClick={() => {
            const canonical = draft.canonical.trim();
            if (!canonical) {
              setError("Informe a grafia correta.");
              return;
            }
            const term = { ...draft, canonical };
            if (editingIndex !== null) {
              const next = [...terms];
              next[editingIndex] = term;
              void persist(next);
            } else {
              void persist([...terms, term]);
            }
          }}
        >
          <Plus className="h-4 w-4" />
          {editingIndex !== null ? "Salvar" : "Adicionar termo"}
        </Button>
      </Card>

      <Input
        placeholder="Buscar por grafia, variação ou categoria…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />

      {loading ? (
        <Card className="p-10 text-center text-sm text-zinc-500">
          Carregando…
        </Card>
      ) : filtered.length === 0 ? (
        <Card className="p-12 text-center text-sm text-zinc-500">
          {query.trim()
            ? "Nenhum termo na busca."
            : "Nenhum termo ainda. Cadastre nomes de arquivo, modelos ou marcas que a fala costuma errar."}
        </Card>
      ) : (
        <div className="space-y-3">
          {filtered.map((t) => {
            const realIdx = terms.indexOf(t);
            return (
              <Card key={`${t.canonical}-${realIdx}`} className="p-5">
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0 space-y-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-mono text-sm text-zinc-100 break-all">
                        {t.canonical}
                      </span>
                      <span className="rounded-full bg-zinc-800 px-2 py-0.5 text-[10px] text-zinc-400">
                        {categoryLabel(t.category)}
                      </span>
                      {t.strict && (
                        <span className="rounded-full bg-coral-500/15 px-2 py-0.5 text-[10px] text-coral-400">
                          Literal
                        </span>
                      )}
                      {!t.enabled && (
                        <span className="text-[10px] text-zinc-500">
                          Pausado
                        </span>
                      )}
                    </div>
                    {t.aliases.length > 0 && (
                      <p className="text-xs text-zinc-500">
                        Variações: {t.aliases.join(" · ")}
                      </p>
                    )}
                  </div>
                  <div className="flex shrink-0 gap-2">
                    <Button
                      variant="secondary"
                      className="text-xs px-3 py-1.5"
                      onClick={() => {
                        setEditingIndex(realIdx);
                        setDraft({ ...t, aliases: [...t.aliases] });
                      }}
                    >
                      Editar
                    </Button>
                    <Button
                      variant="danger"
                      className="text-xs px-3 py-1.5"
                      onClick={() =>
                        void persist(terms.filter((_, i) => i !== realIdx))
                      }
                    >
                      Remover
                    </Button>
                  </div>
                </div>
              </Card>
            );
          })}
        </div>
      )}
    </div>
  );
}

/* ------------------------------ Diagnóstico ------------------------------ */

function DiagnosticoTab() {
  const [devMode, setDevModeState] = useState(false);

  useEffect(() => {
    getDevMode().then(setDevModeState).catch(console.error);
  }, []);

  return (
    <div className="space-y-6">
      <Card className="p-6 space-y-4">
        <div className="flex items-center gap-2">
          <Activity className="h-5 w-5 text-coral-400" />
          <h3 className="text-sm font-semibold text-zinc-100">Diagnóstico</h3>
        </div>
        <Row
          title="Modo desenvolvedor"
          description="No Histórico, botão Request: validador Groq (Ultrapreciso/legado) ou snapshot do pipeline nos outros modos."
        >
          <Toggle
            checked={devMode}
            onChange={(v) => {
              setDevModeState(v);
              setDevMode(v).catch(console.error);
            }}
          />
        </Row>
        <div className="rounded-xl border border-zinc-800/60 bg-zinc-950/40 px-4 py-3 text-xs text-zinc-500 leading-relaxed">
          Logs locais (Windows):{" "}
          <span className="font-mono text-zinc-400">
            %APPDATA%\com.haumeavoice.app\logs\
          </span>
          <br />
          Arquivos <span className="font-mono">app.log</span> e{" "}
          <span className="font-mono">crash.log</span> ajudam a diagnosticar
          travamentos do gadget.
        </div>
      </Card>
    </div>
  );
}

/* --------------------------------- Shared --------------------------------- */

function Row({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-6 px-5 py-5">
      <div className="space-y-1 min-w-0">
        <h3 className="text-sm font-medium text-zinc-100">{title}</h3>
        <p className="text-xs leading-relaxed text-zinc-500">{description}</p>
      </div>
      {children}
    </div>
  );
}
