import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Plus, X } from "lucide-react";
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
  getCustomWords,
  setCustomWords,
  getDevMode,
  setDevMode,
  getSanitizerEnabled,
  setSanitizerEnabled,
  type DeepgramMode,
  type SanitizerModel,
} from "../lib/tauri";
import { Button } from "../components/ui/Button";
import { Card } from "../components/ui/Card";
import { Input } from "../components/ui/Input";
import { Toggle } from "../components/ui/Toggle";

type Tab = "geral" | "motores" | "vocabulario";
type EngineId = "groq-whisper" | "deepgram-nova3" | "gemini-multimodal";
type KeyId = "groq" | "google" | "deepgram";

interface ApiKeys {
  groq: string;
  google: string;
  deepgram: string;
}

const TABS: { key: Tab; label: string }[] = [
  { key: "geral", label: "Geral" },
  { key: "motores", label: "Motores de Nuvem" },
  { key: "vocabulario", label: "Vocabulário" },
];

export function ConfiguracoesView() {
  const [tab, setTab] = useState<Tab>("geral");
  const [activeEngine, setActiveEngine] = useState<EngineId>("groq-whisper");
  const [dualEngine, setDualEngine] = useState(false);
  const [sanitizer, setSanitizer] = useState<SanitizerModel>("llama-70b");
  const [reasoningEnabled, setReasoningEnabled] = useState(false);
  const [reasoningEffort, setReasoningEffort] = useState("medium");
  const [sanitizerEnabled, setSanitizerEnabledState] = useState(true);
  const [deepgramMode, setDeepgramMode] = useState<DeepgramMode>("batch");

  useEffect(() => {
    getEngineConfig()
      .then((cfg) => {
        if (cfg) {
          if (cfg.engine) setActiveEngine(cfg.engine as EngineId);
          if (cfg.dual_engine !== undefined) setDualEngine(cfg.dual_engine);
          if (cfg.sanitizer) setSanitizer(cfg.sanitizer as SanitizerModel);
          if (cfg.reasoning_enabled !== undefined) setReasoningEnabled(cfg.reasoning_enabled);
          if (cfg.reasoning_effort) setReasoningEffort(cfg.reasoning_effort);
          if (cfg.deepgram_mode) setDeepgramMode(cfg.deepgram_mode);
        }
      })
      .catch((e) => console.error("getEngineConfig failed:", e));
    getSanitizerEnabled()
      .then(setSanitizerEnabledState)
      .catch((e) => console.error("getSanitizerEnabled failed:", e));
  }, []);

  const handleToggleSanitizer = (val: boolean) => {
    setSanitizerEnabledState(val);
    setSanitizerEnabled(val).catch((e) => console.error("set_sanitizer_enabled failed:", e));
  };

  const handleUpdateConfig = (
    updates: Partial<{
      engine: EngineId;
      sanitizer: SanitizerModel;
      dual_engine: boolean;
      reasoning_enabled: boolean;
      reasoning_effort: string;
      deepgram_mode: DeepgramMode;
    }>,
  ) => {
    const payload = {
      engine: updates.engine ?? activeEngine,
      sanitizer: updates.sanitizer ?? sanitizer,
      dual_engine: updates.dual_engine ?? dualEngine,
      reasoning_enabled: updates.reasoning_enabled ?? reasoningEnabled,
      reasoning_effort: updates.reasoning_effort ?? reasoningEffort,
      deepgram_mode: updates.deepgram_mode ?? deepgramMode,
    };
    invoke("update_engine_config", { payload }).catch((e) => console.error("update_engine_config failed:", e));
  };

  const handleSelectEngine = (id: EngineId) => {
    setActiveEngine(id);
    handleUpdateConfig({ engine: id });
  };

  const handleToggleDualEngine = (val: boolean) => {
    setDualEngine(val);
    handleUpdateConfig({ dual_engine: val });
  };

  const handleSanitizerChange = (val: SanitizerModel) => {
    setSanitizer(val);
    handleUpdateConfig({ sanitizer: val });
  };

  const handleReasoningToggle = (val: boolean) => {
    setReasoningEnabled(val);
    handleUpdateConfig({ reasoning_enabled: val });
  };

  const handleReasoningEffortChange = (val: string) => {
    setReasoningEffort(val);
    handleUpdateConfig({ reasoning_effort: val });
  };

  const handleDeepgramModeChange = (mode: DeepgramMode) => {
    setDeepgramMode(mode);
    handleUpdateConfig({ deepgram_mode: mode });
  };

  return (
    <div className="space-y-8">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight text-zinc-100">
          Configurações
        </h1>
        <p className="mt-1 text-sm text-zinc-500">
          Personalize o comportamento e os motores do Haumea Voice.
        </p>
      </header>

      {/* Sub-abas */}
      <div className="flex gap-1 border-b border-zinc-800/60">
        {TABS.map((t) => (
          <button
            key={t.key}
            onClick={() => setTab(t.key)}
            className={
              "relative px-5 py-3 text-sm font-medium transition-colors duration-200 " +
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
      {tab === "motores" && (
        <MotoresTab
          activeEngine={activeEngine}
          onSelect={handleSelectEngine}
          dualEngine={dualEngine}
          onToggleDualEngine={handleToggleDualEngine}
          sanitizer={sanitizer}
          onSanitizerChange={handleSanitizerChange}
          reasoningEnabled={reasoningEnabled}
          onReasoningToggle={handleReasoningToggle}
          reasoningEffort={reasoningEffort}
          onReasoningEffortChange={handleReasoningEffortChange}
          sanitizerEnabled={sanitizerEnabled}
          onToggleSanitizer={handleToggleSanitizer}
          deepgramMode={deepgramMode}
          onDeepgramModeChange={handleDeepgramModeChange}
        />
      )}
      {tab === "vocabulario" && <VocabularioTab />}
    </div>
  );
}

/* ----------------------------- Icons (SVG inline) ----------------------------- */

function IconBolt({ className = "" }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    >
      <path d="M13 2 4 14h7l-1 8 9-12h-7l1-8z" />
    </svg>
  );
}

function IconShield({ className = "" }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    >
      <path d="M12 2 4 5v6c0 5 3.4 9.4 8 11 4.6-1.6 8-6 8-11V5l-8-3z" />
    </svg>
  );
}

function IconSparkles({ className = "" }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    >
      <path d="M12 3v4M12 17v4M3 12h4M17 12h4M5.6 5.6l2.8 2.8M15.6 15.6l2.8 2.8M18.4 5.6l-2.8 2.8M8.4 15.6l-2.8 2.8" />
    </svg>
  );
}

function IconKey({ className = "" }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    >
      <circle cx="7.5" cy="15.5" r="4.5" />
      <path d="M10.5 12.5 21 2M17 6l3 3M14 9l3 3" />
    </svg>
  );
}

function IconEye({ className = "" }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    >
      <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  );
}

function IconEyeOff({ className = "" }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    >
      <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-6.5 0-10-7-10-7a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c6.5 0 10 7 10 7a18.5 18.5 0 0 1-2.16 3.19M1 1l22 22M9.88 9.88a3 3 0 0 0 4.24 4.24" />
    </svg>
  );
}

function IconSave({ className = "" }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    >
      <path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z" />
      <path d="M17 21v-8H7v8M7 3v5h8" />
    </svg>
  );
}

function IconMic({ className = "" }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    >
      <rect x="9" y="2" width="6" height="12" rx="3" />
      <path d="M19 10a7 7 0 0 1-14 0M12 19v3M8 22h8" />
    </svg>
  );
}

/* --------------------------------- Geral --------------------------------- */

function GeralTab() {
  const [startup, setStartup] = useState(false);
  const [compact, setCompact] = useState(false);
  const [tray, setTray] = useState(true);
  const [devMode, setDevModeState] = useState(false);

  // Estados do microfone
  const [devices, setDevices] = useState<string[]>([]);
  const [selectedDevice, setSelectedDevice] = useState<string | null>(null);
  const [isTesting, setIsTesting] = useState(false);
  const [micLevel, setMicLevel] = useState(0);

  // Carregar compact-mode, autostart e dispositivos de áudio
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

    getDevMode()
      .then(setDevModeState)
      .catch((e) => console.error("getDevMode failed:", e));
  }, []);

  // Escutar o nível do microfone quando estiver testando
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
        if (active) {
          unlisten = unsub;
        } else {
          // O componente desmontou (ou o teste foi parado) antes da
          // inscrição concluir: removemos o listener imediatamente para
          // evitar acúmulo de listeners órfãos que travam a UI.
          try {
            unsub();
          } catch {
            /* noop */
          }
        }
      })
      .catch((e) => console.error("onMicTestLevel failed:", e));

    return () => {
      active = false;
      if (unlisten) {
        try {
          unlisten();
        } catch {
          /* noop */
        }
        unlisten = null;
      }
    };
  }, [isTesting]);

  // Parar o teste ao desmontar
  useEffect(() => {
    return () => {
      stopMicTest().catch((e) => console.error("stopMicTest on unmount failed:", e));
    };
  }, []);

  const handleStartup = (value: boolean) => {
    setStartup(value);
    invoke("set_autostart", { enabled: value }).catch((e) =>
      console.error("set_autostart failed:", e),
    );
  };

  const handleCompact = (value: boolean) => {
    setCompact(value);
    setCompactMode(value).catch((e) =>
      console.error("set_compact_mode failed:", e),
    );
  };

  const handleDevMode = (value: boolean) => {
    setDevModeState(value);
    setDevMode(value).catch((e) => console.error("set_dev_mode failed:", e));
  };

  const handleDeviceChange = async (device: string) => {
    const val = device === "default" ? null : device;
    setSelectedDevice(val);
    try {
      await setInputDevice(val);
      const list = await listAudioDevices();
      setDevices(list);
    } catch (e) {
      console.error("setInputDevice failed:", e);
    }
  };

  const handleToggleTest = async () => {
    if (isTesting) {
      try {
        await stopMicTest();
        setIsTesting(false);
      } catch (e) {
        console.error("stopMicTest failed:", e);
      }
    } else {
      try {
        await startMicTest();
        setIsTesting(true);
      } catch (e) {
        console.error("startMicTest failed:", e);
      }
    }
  };

  return (
    <div className="space-y-6">
      <Card className="divide-y divide-zinc-800/60 p-2">
        <Row
          title="Inicialização com o Windows"
          description="Inicia o Haumea Voice automaticamente ao ligar o computador."
        >
          <Toggle checked={startup} onChange={handleStartup} />
        </Row>
        <Row
          title="Modo Compacto do Gadget"
          description="Com o modo ativo, o gadget flutuante fica reduzido a um ícone enquanto ocioso e só se expande ao gravar. Desativado, mostra o ícone com o nome “Haumea Voice”."
        >
          <Toggle checked={compact} onChange={handleCompact} />
        </Row>
        <Row
          title="Minimizar para a bandeja"
          description="Mantém o app em execução na bandeja do sistema ao fechar a janela."
        >
          <Toggle checked={tray} onChange={setTray} />
        </Row>
        <Row
          title="Modo Desenvolvedor"
          description="Exibe no Histórico um botão para inspecionar o request enviado ao validador semântico: modelo, parâmetros, nível de reasoning aplicado, prompt e o JSON completo da requisição."
        >
          <Toggle checked={devMode} onChange={handleDevMode} />
        </Row>
      </Card>

      {/* Dispositivo de Entrada */}
      <Card className="p-7 space-y-6">
        <div className="flex items-center gap-2.5">
          <IconMic className="h-5 w-5 text-coral-400" />
          <h3 className="text-sm font-semibold text-zinc-100">
            Dispositivo de Entrada
          </h3>
        </div>
        
        <p className="text-xs text-zinc-500 leading-relaxed">
          Selecione o microfone que deseja utilizar para captura das gravações. 
          O Haumea Voice lembrará de sua escolha. Caso o dispositivo escolhido não seja encontrado, 
          ele usará o microfone padrão do sistema operacional de forma inteligente.
        </p>

        <div className="flex flex-col sm:flex-row gap-4 items-start sm:items-center">
          <div className="relative w-full max-w-md">
            <select
              value={selectedDevice || "default"}
              onChange={(e) => handleDeviceChange(e.target.value)}
              className="w-full bg-zinc-900 border border-zinc-800/80 text-zinc-200 text-xs rounded-xl px-4 py-3 outline-none focus:border-coral-500/40 hover:border-zinc-700 transition-colors appearance-none cursor-pointer"
            >
              <option value="default">Padrão do Sistema Operacional</option>
              {devices.map((device) => (
                <option key={device} value={device}>
                  {device}
                </option>
              ))}
            </select>
            <div className="pointer-events-none absolute right-4 top-1/2 -translate-y-1/2 text-zinc-500">
              <svg className="h-4 w-4 fill-none stroke-current stroke-2" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" d="m19.5 8.25-7.5 7.5-7.5-7.5" />
              </svg>
            </div>
          </div>

          <Button
            variant={isTesting ? "secondary" : "primary"}
            onClick={handleToggleTest}
            className="w-full sm:w-auto h-[42px] px-6 text-xs gap-2 font-medium"
          >
            <span className={`h-2 w-2 rounded-full ${isTesting ? "bg-red-500 animate-pulse" : "bg-zinc-500"}`} />
            {isTesting ? "Parar Teste" : "Testar Microfone"}
          </Button>
        </div>

        {/* Visualizador de Teste de Microfone */}
        {isTesting && (
          <div className="space-y-2 animate-gadget-pop">
            <div className="flex justify-between items-center text-[10px] uppercase tracking-wider font-semibold text-zinc-500">
              <span>Nível de Entrada</span>
              <span className="font-mono text-zinc-400">{Math.round(micLevel * 100)}%</span>
            </div>
            
            <div className="h-2 w-full bg-zinc-900/60 rounded-full overflow-hidden border border-zinc-800/40">
              <div
                className="h-full bg-gradient-to-r from-coral-500 via-coral-400 to-amber-400 rounded-full transition-all duration-75 ease-out"
                style={{ width: `${micLevel * 100}%` }}
              />
            </div>
          </div>
        )}
      </Card>
    </div>
  );
}

/* -------------------------------- Motores -------------------------------- */

type Engine = {
  id: EngineId;
  name: string;
  tag: string;
  desc: string;
  Icon: (p: { className?: string }) => JSX.Element;
  /** Which stored API key this engine uses. */
  keyId: KeyId;
  keyPlaceholder: string;
  keyHelp: string;
  /**
   * When true, this entry is not a selectable transcription engine — it is
   * only consumed internally for a specific feature. The card hides the
   * "Selecionar" button and shows an informational badge instead.
   */
  evaluationOnly?: boolean;
};

const ENGINES: Engine[] = [
  {
    id: "groq-whisper",
    name: "Groq Whisper",
    tag: "Padrão de Velocidade — Sub-segundo",
    desc: "Inferência ultra-rápida em LPU, ideal para transcrições em tempo real e fluxos de baixa latência.",
    Icon: IconBolt,
    keyId: "groq",
    keyPlaceholder: "gsk_...",
    keyHelp: "Usada na transcrição Whisper e no validador semântico.",
  },
  {
    id: "deepgram-nova3",
    name: "Deepgram Nova-3",
    tag: "Backup de Alta Estabilidade",
    desc: "Modelo de reconhecimento de fala de última geração com forte precisão em ambientes ruidosos e multilíngues.",
    Icon: IconShield,
    keyId: "deepgram",
    keyPlaceholder: "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
    keyHelp: "Usada na transcrição Deepgram Nova-3.",
  },
  {
    id: "gemini-multimodal",
    name: "Gemini Multimodal",
    tag: "Avaliação de Pronúncia",
    desc: "Modelo multimodal nativo que analisa o áudio das suas gravações no Histórico e devolve um relatório de proficiência oral (CEFR). Não é um motor de transcrição.",
    Icon: IconSparkles,
    keyId: "google",
    keyPlaceholder: "AIza...",
    keyHelp: "Usada apenas na avaliação de pronúncia do Histórico.",
    evaluationOnly: true,
  },
];

function MotoresTab({
  activeEngine,
  onSelect,
  dualEngine,
  onToggleDualEngine,
  sanitizer,
  onSanitizerChange,
  reasoningEnabled,
  onReasoningToggle,
  reasoningEffort,
  onReasoningEffortChange,
  sanitizerEnabled,
  onToggleSanitizer,
  deepgramMode,
  onDeepgramModeChange,
}: {
  activeEngine: EngineId;
  onSelect: (id: EngineId) => void;
  dualEngine: boolean;
  onToggleDualEngine: (val: boolean) => void;
  sanitizer: SanitizerModel;
  onSanitizerChange: (val: SanitizerModel) => void;
  reasoningEnabled: boolean;
  onReasoningToggle: (val: boolean) => void;
  reasoningEffort: string;
  onReasoningEffortChange: (val: string) => void;
  sanitizerEnabled: boolean;
  onToggleSanitizer: (val: boolean) => void;
  deepgramMode: DeepgramMode;
  onDeepgramModeChange: (mode: DeepgramMode) => void;
}) {
  const [keys, setKeys] = useState<ApiKeys>({ groq: "", google: "", deepgram: "" });
  const [visible, setVisible] = useState<Record<KeyId, boolean>>({
    groq: false,
    google: false,
    deepgram: false,
  });
  const [savingKey, setSavingKey] = useState<KeyId | null>(null);
  const [savedKey, setSavedKey] = useState<KeyId | null>(null);

  // Reasoning is only honoured natively by the GPT-OSS family on Groq; LLaMA
  // 70B has no native reasoning mode, so the controls are hidden for it.
  const supportsReasoning = sanitizer === "gpt-oss-20b" || sanitizer === "gpt-oss-120b";

  // Prefill from the persisted keys saved by the backend (api_keys.json).
  useEffect(() => {
    invoke<{ groq?: string | null; google?: string | null; deepgram?: string | null }>(
      "get_api_keys",
    )
      .then((k) =>
        setKeys({ groq: k.groq ?? "", google: k.google ?? "", deepgram: k.deepgram ?? "" }),
      )
      .catch((e) => console.error("get_api_keys failed:", e));
  }, []);

  // `save_api_keys` replaces all keys atomically, so we always send the full
  // current trio regardless of which card triggered the save.
  const handleSaveKey = async (id: KeyId) => {
    setSavingKey(id);
    try {
      await invoke("save_api_keys", {
        payload: { groq: keys.groq, google: keys.google, deepgram: keys.deepgram },
      });
      setSavedKey(id);
      window.setTimeout(() => setSavedKey((cur) => (cur === id ? null : cur)), 2000);
    } catch (e) {
      console.error("save_api_keys failed:", e);
    } finally {
      setSavingKey(null);
    }
  };

  return (
    <div className="space-y-6">
      {/* Banner / Card de Modo Dual */}
      <Card className={`p-6 border transition-all duration-200 ${dualEngine ? "border-coral-500/60 bg-coral-500/5 shadow-[0_0_24px_-8px_rgba(225,77,42,0.3)]" : "border-zinc-800/60"}`}>
        <div className="flex items-center justify-between gap-6">
          <div className="space-y-1">
            <div className="flex items-center gap-2">
              <span className={`h-2 w-2 rounded-full ${dualEngine ? "bg-coral-500 animate-pulse" : "bg-zinc-600"}`} />
              <h3 className="text-sm font-semibold text-zinc-100">Modo Motor Duplo (Paralelo)</h3>
            </div>
            <p className="text-xs text-zinc-500 leading-relaxed max-w-xl">
              Ao ativar, o app roda <strong>Groq Whisper</strong> e <strong>Deepgram Nova-3</strong> no mesmo áudio (mic ou arquivo). Com Streaming Final, o Deepgram processa <em>durante</em> a gravação; o Whisper só começa ao parar — o dual espera os dois e o validador mescla o melhor texto. Sem validador, o app escolhe o melhor texto bruto (não só o Whisper). A latência do dual fica limitada pelo motor mais lento.
            </p>
          </div>
          <div className="flex items-center gap-3 shrink-0">
            <span className="text-xs font-medium text-zinc-400">
              {dualEngine ? "Ativado" : "Desativado"}
            </span>
            <Toggle checked={dualEngine} onChange={onToggleDualEngine} />
          </div>
        </div>
      </Card>

      {/* Validador Semântico */}
      <Card className={`p-6 border-zinc-800/60 divide-y divide-zinc-800/60 transition-colors duration-200 ${sanitizerEnabled ? "" : "border-zinc-800/40 bg-zinc-950/30"}`}>
        <div className="pb-5">
          <div className="flex items-center justify-between gap-4 mb-2">
            <div className="flex items-center gap-2">
              <IconSparkles className={`h-5 w-5 ${sanitizerEnabled ? "text-coral-400" : "text-zinc-600"}`} />
              <h3 className="text-sm font-semibold text-zinc-100">Validador Semântico (Sanitização)</h3>
            </div>
            <div className="flex items-center gap-3 shrink-0">
              <span className="text-xs font-medium text-zinc-400">
                {sanitizerEnabled ? "Ativado" : "Desativado"}
              </span>
              <Toggle checked={sanitizerEnabled} onChange={onToggleSanitizer} />
            </div>
          </div>
          <p className="text-xs text-zinc-500 leading-relaxed mb-4">
            Escolha o modelo de Inteligência Artificial que revisará a transcrição para garantir ortografia impecável, formatação de números e correção contextual.
          </p>
          <div className={`grid grid-cols-2 gap-3 transition-opacity duration-200 ${sanitizerEnabled ? "" : "opacity-40 pointer-events-none"}`}>
            <Button
              variant={sanitizer === "llama-70b" ? "secondary" : "primary"}
              onClick={() => onSanitizerChange("llama-70b")}
              className={`${sanitizer === "llama-70b" ? "border-coral-500/50 bg-coral-500/10 text-coral-400" : ""}`}
            >
              LLaMA 70B
            </Button>
            <Button
              variant={sanitizer === "gpt-oss-20b" ? "secondary" : "primary"}
              onClick={() => onSanitizerChange("gpt-oss-20b")}
              className={`${sanitizer === "gpt-oss-20b" ? "border-coral-500/50 bg-coral-500/10 text-coral-400" : ""}`}
            >
              GPT-OSS 20B
            </Button>
            <Button
              variant={sanitizer === "gpt-oss-120b" ? "secondary" : "primary"}
              onClick={() => onSanitizerChange("gpt-oss-120b")}
              className={`${sanitizer === "gpt-oss-120b" ? "border-coral-500/50 bg-coral-500/10 text-coral-400" : ""}`}
            >
              GPT-OSS 120B
            </Button>
            <Button
              variant={sanitizer === "qwen3-27b" ? "secondary" : "primary"}
              onClick={() => onSanitizerChange("qwen3-27b")}
              className={`${sanitizer === "qwen3-27b" ? "border-coral-500/50 bg-coral-500/10 text-coral-400" : ""}`}
            >
              Qwen 3.6 27B
            </Button>
          </div>

          {!sanitizerEnabled && (
            <p className="mt-3 text-[11px] leading-relaxed text-amber-400/70 border border-amber-500/20 bg-amber-500/5 rounded-lg px-3 py-2">
              Com o validador desligado, a transcrição acústica bruta é enviada diretamente para a área de transferência sem revisão ortográfica ou formatação. O modelo selecionado acima será usado quando você reativar o validador.
            </p>
          )}

          <div className={`mt-4 pt-4 border-t border-zinc-800/60 space-y-4 transition-opacity duration-200 ${sanitizerEnabled ? "" : "opacity-40 pointer-events-none"}`}>
            {supportsReasoning ? (
              <>
                <div className="flex items-center justify-between">
                  <div className="space-y-1">
                    <h4 className="text-sm font-medium text-zinc-100">Ativar Reasoning</h4>
                    <p className="text-xs text-zinc-500">
                      Usa o parâmetro nativo <span className="font-mono text-zinc-400">reasoning_effort</span> dos modelos GPT-OSS antes de responder, aumentando a qualidade da validação de transcrições longas. Sem ativar, o modelo usa o esforço padrão (médio).
                    </p>
                  </div>
                  <Toggle checked={reasoningEnabled} onChange={onReasoningToggle} />
                </div>

                {reasoningEnabled && (
                  <div className="flex flex-col gap-2">
                    <label className="text-[11px] font-medium uppercase tracking-wider text-zinc-500">Esforço do Reasoning</label>
                    <div className="flex gap-2">
                      {["low", "medium", "high"].map((effort) => (
                        <button
                          key={effort}
                          onClick={() => onReasoningEffortChange(effort)}
                          className={`flex-1 py-2 rounded-xl text-xs font-medium transition-all duration-200 border ${
                            reasoningEffort === effort
                              ? "bg-coral-500 text-white border-coral-500 shadow-sm"
                              : "bg-zinc-900 border-zinc-800 text-zinc-400 hover:bg-zinc-800"
                          }`}
                        >
                          {effort === "low" ? "Baixo" : effort === "medium" ? "Médio" : "Alto"}
                        </button>
                      ))}
                    </div>
                  </div>
                )}
              </>
            ) : (
              <div className="space-y-1">
                <h4 className="text-sm font-medium text-zinc-100">Reasoning</h4>
                <p className="text-xs text-zinc-500">
                  O modelo selecionado (<strong>{sanitizer === "qwen3-27b" ? "Qwen 3.6 27B" : "LLaMA 70B"}</strong>) não possui reasoning nativo no Groq. Selecione um modelo <strong>GPT-OSS</strong> (20B ou 120B) para habilitar o controle de esforço de raciocínio.
                </p>
              </div>
            )}
          </div>
        </div>
      </Card>

      <div className="grid grid-cols-3 gap-5">
        {ENGINES.map((e) => {
          const isActive = activeEngine === e.id && !dualEngine;
          const isDualActive = dualEngine && (e.id === "groq-whisper" || e.id === "deepgram-nova3");
          const isEvalOnly = Boolean(e.evaluationOnly);
          const Icon = e.Icon;
          return (
            <Card
              key={e.id}
              className={
                "flex flex-col p-6 transition-all duration-200 " +
                (isActive || isDualActive
                  ? "border-coral-500/60 bg-coral-500/5 shadow-[0_0_24px_-8px_rgba(225,77,42,0.4)]"
                  : "border-zinc-800/60 hover:border-zinc-700 hover:bg-zinc-900/80")
              }
            >
              <div className="flex items-start justify-between">
                <div
                  className={
                    "flex h-11 w-11 items-center justify-center rounded-xl transition-colors duration-200 " +
                    (isActive || isDualActive
                      ? "bg-coral-500/15 text-coral-400"
                      : "bg-zinc-800 text-zinc-400")
                  }
                >
                  <Icon className="h-5 w-5" />
                </div>
                {isEvalOnly && (
                  <span className="inline-flex items-center gap-1.5 rounded-full bg-zinc-800 px-3 py-1 text-[11px] font-medium text-zinc-400">
                    <IconSparkles className="h-3 w-3 text-coral-400" />
                    Apenas avaliação
                  </span>
                )}
                {isDualActive && (
                  <span className="inline-flex items-center gap-1.5 rounded-full bg-coral-500/15 px-3 py-1 text-[11px] font-medium text-coral-400">
                    <span className="h-1.5 w-1.5 rounded-full bg-coral-400 animate-pulse" />
                    Ativo (Modo Duplo)
                  </span>
                )}
                {isActive && (
                  <span className="inline-flex items-center gap-1.5 rounded-full bg-coral-500/15 px-3 py-1 text-[11px] font-medium text-coral-400">
                    <span className="h-1.5 w-1.5 rounded-full bg-coral-500" />
                    Ativo
                  </span>
                )}
              </div>

              <h3 className="mt-5 text-base font-semibold text-zinc-100">
                {e.name}
              </h3>
              <p className="mt-1 text-[11px] font-medium uppercase tracking-wider text-coral-400/80">
                {e.tag}
              </p>
              <p className="mt-2 flex-1 text-sm leading-relaxed text-zinc-500">
                {e.desc}
              </p>

              {/* Chave de API específica deste motor */}
              <div className="mt-5 space-y-2">
                <label className="flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-wider text-zinc-500">
                  <IconKey className="h-3.5 w-3.5 text-coral-400" /> Chave de API
                </label>
                <div className="relative">
                  <Input
                    type={visible[e.keyId] ? "text" : "password"}
                    placeholder={e.keyPlaceholder}
                    value={keys[e.keyId]}
                    onChange={(ev) =>
                      setKeys((k) => ({ ...k, [e.keyId]: ev.target.value }))
                    }
                    autoComplete="off"
                    spellCheck={false}
                    className="pr-10 text-xs"
                  />
                  <button
                    type="button"
                    onClick={() =>
                      setVisible((v) => ({ ...v, [e.keyId]: !v[e.keyId] }))
                    }
                    className="absolute right-3 top-1/2 -translate-y-1/2 text-zinc-500 transition-colors hover:text-zinc-300"
                    aria-label={visible[e.keyId] ? "Ocultar chave" : "Mostrar chave"}
                  >
                    {visible[e.keyId] ? (
                      <IconEyeOff className="h-4 w-4" />
                    ) : (
                      <IconEye className="h-4 w-4" />
                    )}
                  </button>
                </div>
                <p className="text-[11px] leading-relaxed text-zinc-600">
                  {e.keyHelp}
                </p>
                <Button
                  variant="secondary"
                  className="w-full gap-2 text-xs"
                  onClick={() => handleSaveKey(e.keyId)}
                  disabled={savingKey === e.keyId}
                >
                  <IconSave className="h-3.5 w-3.5" />
                  {savingKey === e.keyId
                    ? "Salvando..."
                    : savedKey === e.keyId
                      ? "Chave salva!"
                      : "Salvar Chave"}
                </Button>
              </div>

              {e.id === "deepgram-nova3" && (
                <div className="mt-4 space-y-2">
                  <label className="text-[11px] font-medium uppercase tracking-wider text-zinc-500">
                    Modo de Transcrição
                  </label>
                  <div className="flex gap-2">
                    {(
                      [
                        {
                          id: "batch" as const,
                          label: "Batch",
                          title: "Envia o arquivo completo via REST",
                        },
                        {
                          id: "streaming_final" as const,
                          label: "Streaming Final",
                          title:
                            "WebSocket em chunks; retorna só o resultado final (sem parciais na UI)",
                        },
                      ] as const
                    ).map((opt) => (
                      <button
                        key={opt.id}
                        type="button"
                        title={opt.title}
                        onClick={() => onDeepgramModeChange(opt.id)}
                        className={`flex-1 py-2 rounded-xl text-xs font-medium transition-all duration-200 border ${
                          deepgramMode === opt.id
                            ? "bg-coral-500 text-white border-coral-500 shadow-sm"
                            : "bg-zinc-900 border-zinc-800 text-zinc-400 hover:bg-zinc-800"
                        }`}
                      >
                        {opt.label}
                      </button>
                    ))}
                  </div>
                  <p className="text-[11px] leading-relaxed text-zinc-600">
                    {deepgramMode === "streaming_final"
                      ? "Streaming processa o áudio em chunks após gravar e devolve apenas o texto final — em geral mais rápido que o batch."
                      : "Batch envia o WAV completo de uma vez pela API REST (comportamento clássico)."}
                  </p>
                </div>
              )}

              {isEvalOnly ? (
                <div className="mt-4 flex items-center justify-center gap-2 rounded-xl border border-zinc-800/60 bg-zinc-950/40 px-4 py-2.5 text-center text-[11px] font-medium text-zinc-500">
                  <IconSparkles className="h-3.5 w-3.5 text-coral-400" />
                  Disponível em Histórico › Avaliar Pronúncia
                </div>
              ) : (
                <Button
                  variant={isActive || isDualActive ? "secondary" : "primary"}
                  className="mt-4 w-full"
                  onClick={() => onSelect(e.id)}
                  disabled={isActive || isDualActive || (dualEngine && e.id === "gemini-multimodal")}
                >
                  {isActive ? "Selecionado" : isDualActive ? "Ativo (Modo Duplo)" : "Selecionar"}
                </Button>
              )}
            </Card>
          );
        })}
      </div>
    </div>
  );
}

/* ------------------------------- Vocabulário ------------------------------ */

function VocabularioTab() {
  const [words, setWords] = useState<string[]>([]);
  const [draft, setDraft] = useState("");
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    getCustomWords()
      .then(setWords)
      .catch((e) => console.error("getCustomWords failed:", e))
      .finally(() => setLoading(false));
  }, []);

  // Persists `next` optimistically, then re-syncs with the backend's
  // normalised result (trimmed, de-blanked, case-insensitively deduped).
  const persist = (next: string[]) => {
    setWords(next);
    setCustomWords(next)
      .then(setWords)
      .catch((e) => console.error("setCustomWords failed:", e));
  };

  const handleAdd = () => {
    const w = draft.trim();
    if (!w) return;
    const exists = words.some((x) => x.toLowerCase() === w.toLowerCase());
    if (!exists) persist([...words, w]);
    setDraft("");
  };

  const handleRemove = (word: string) =>
    persist(words.filter((w) => w !== word));

  return (
    <div className="space-y-6">
      {/* Cabeçalho explicativo */}
      <Card className="p-6 border-zinc-800/60">
        <div className="flex items-center gap-2 mb-2">
          <IconSparkles className="h-5 w-5 text-coral-400" />
          <h3 className="text-sm font-semibold text-zinc-100">
            Vocabulário Personalizado
          </h3>
        </div>
        <p className="text-xs text-zinc-500 leading-relaxed">
          Cadastre nomes, marcas, termos técnicos ou palavras que você usa com
          frequência e que a transcrição costuma errar. Durante a sanitização, o
          validador semântico corrige automaticamente qualquer palavra que soe
          ou se pareça muito com uma destas, trocando-a pela grafia exata que
          você cadastrou. A correção é conservadora: na dúvida, o texto original
          é mantido.
        </p>
      </Card>

      {/* Formulário de adição */}
      <Card className="p-6">
        <label className="text-[11px] font-medium uppercase tracking-wider text-zinc-500">
          Adicionar palavra
        </label>
        <div className="mt-2 flex gap-3">
          <Input
            placeholder="ex: Haumea, Kubernetes, PostgreSQL"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                handleAdd();
              }
            }}
            autoComplete="off"
            spellCheck={false}
            className="flex-1"
          />
          <Button
            variant="primary"
            className="shrink-0 gap-2"
            onClick={handleAdd}
            disabled={!draft.trim()}
          >
            <Plus className="h-4 w-4" />
            Adicionar
          </Button>
        </div>
      </Card>

      {/* Lista de palavras / estado vazio */}
      {loading ? (
        <Card className="p-10 text-center">
          <p className="text-sm text-zinc-500">Carregando...</p>
        </Card>
      ) : words.length === 0 ? (
        <Card className="p-12 text-center">
          <p className="text-sm text-zinc-500">
            Nenhuma palavra cadastrada. Adicione a primeira acima para começar a
            guiar a correção.
          </p>
        </Card>
      ) : (
        <Card className="p-6">
          <div className="mb-4 flex items-center justify-between">
            <span className="text-[11px] font-medium uppercase tracking-wider text-zinc-500">
              {words.length} {words.length === 1 ? "palavra" : "palavras"}
            </span>
          </div>
          <div className="flex flex-wrap gap-2">
            {words.map((w) => (
              <span
                key={w}
                className="group inline-flex items-center gap-2 rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-sm text-zinc-200"
              >
                <span className="font-mono">{w}</span>
                <button
                  onClick={() => handleRemove(w)}
                  className="text-zinc-600 transition-colors hover:text-red-400"
                  aria-label={`Remover ${w}`}
                >
                  <X className="h-3.5 w-3.5" />
                </button>
              </span>
            ))}
          </div>
        </Card>
      )}
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
      <div className="space-y-1">
        <h3 className="text-sm font-medium text-zinc-100">{title}</h3>
        <p className="text-xs leading-relaxed text-zinc-500">{description}</p>
      </div>
      {children}
    </div>
  );
}
