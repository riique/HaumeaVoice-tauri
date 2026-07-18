import { useEffect, useState } from "react";
import { Card } from "../components/ui/Card";
import { Button } from "../components/ui/Button";
import { KbdCombo } from "../components/ui/Kbd";
import { getShortcuts, setShortcuts, type ShortcutConfig } from "../lib/tauri";

type BindId = "toggle" | "cancel";

const BIND_META: { id: BindId; title: string; description: string }[] = [
  {
    id: "toggle",
    title: "Iniciar / Alternar Gravação",
    description: "Inicia uma nova gravação ou para a gravação ativa atual.",
  },
  {
    id: "cancel",
    title: "Cancelar Gravação",
    description: "Descarta a gravação em andamento sem salvar a transcrição.",
  },
];

/** Converts the backend shortcut string ("Control+Shift+B") into the tokens
 *  used by KbdCombo, abbreviating modifier names for display. */
function toKeys(shortcut: string): string[] {
  const labels: Record<string, string> = {
    Control: "Ctrl",
    CommandOrControl: "Ctrl",
    Alt: "Alt",
    Shift: "Shift",
    Super: "Win",
    Meta: "Win",
  };
  return shortcut.split("+").map((k) => labels[k] ?? k);
}

/** Translates a browser KeyboardEvent into the global-hotkey string format
 *  understood by the backend. Returns null for incomplete combos (e.g. only a
 *  modifier pressed) or unsupported keys. */
function eventToShortcut(e: React.KeyboardEvent): string | null {
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Control");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");

  const key = e.key;
  if (["Control", "Alt", "Shift", "Meta", "Dead"].includes(key)) return null;

  let main = "";
  if (/^[a-zA-Z]$/.test(key)) main = key.toUpperCase();
  else if (/^[0-9]$/.test(key)) main = key;
  else if (/^F([1-9]|1[0-2])$/.test(key)) main = key;
  else {
    const map: Record<string, string> = {
      " ": "Space",
      ArrowUp: "Up",
      ArrowDown: "Down",
      ArrowLeft: "Left",
      ArrowRight: "Right",
      Enter: "Enter",
      Escape: "Escape",
      Tab: "Tab",
      Backspace: "Backspace",
      ",": "Comma",
      ".": "Period",
      "/": "Slash",
      ";": "Semicolon",
    };
    main = map[key] ?? "";
  }
  if (!main) return null;

  // Require a modifier for letters/digits so a plain key never hijacks typing
  // globally. F-keys are allowed standalone.
  const isFKey = /^F([1-9]|1[0-2])$/.test(main);
  if (mods.length === 0 && !isFKey) return null;

  return [...mods, main].join("+");
}

export function AtalhosView() {
  const [cfg, setCfg] = useState<ShortcutConfig>({ toggle: "Control+B", cancel: "Control+Q" });
  const [capturing, setCapturing] = useState<BindId | null>(null);
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    getShortcuts()
      .then(setCfg)
      .catch((e) => console.error("get_shortcuts failed:", e));
  }, []);

  const handleKeyDown = async (bind: BindId, e: React.KeyboardEvent) => {
    e.preventDefault();
    if (e.key === "Escape") {
      setCapturing(null);
      return;
    }
    const combo = eventToShortcut(e);
    if (!combo) {
      setError("Use ao menos um modificador (Ctrl, Alt ou Shift) com a tecla.");
      return;
    }

    const next: ShortcutConfig =
      bind === "toggle"
        ? { toggle: combo, cancel: cfg.cancel }
        : { toggle: cfg.toggle, cancel: combo };

    setSaving(true);
    setError("");
    try {
      const applied = await setShortcuts(next.toggle, next.cancel);
      setCfg(applied);
      setCapturing(null);
    } catch (err) {
      setError(typeof err === "string" ? err : String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-8">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight text-zinc-100">
          Atalhos
        </h1>
        <p className="mt-1 text-sm text-zinc-500">
          Atalhos globais de teclado disponíveis em qualquer parte do sistema.
          Clique em "Alterar" e pressione a nova combinação.
        </p>
      </header>

      {error && (
        <div className="rounded-xl border border-red-500/30 bg-red-500/5 px-4 py-3 text-sm text-red-400">
          {error}
        </div>
      )}

      <div className="grid grid-cols-2 gap-5">
        {BIND_META.map((b) => {
          const isCapturing = capturing === b.id;
          const combo = cfg[b.id];
          return (
            <Card key={b.id} className="p-7">
              <div className="space-y-5">
                <div>
                  <h3 className="text-base font-semibold text-zinc-100">
                    {b.title}
                  </h3>
                  <p className="mt-1 text-sm leading-relaxed text-zinc-500">
                    {b.description}
                  </p>
                </div>

                <div
                  className={
                    "flex items-center justify-between rounded-xl border px-5 py-4 transition-colors " +
                    (isCapturing
                      ? "border-coral-500/60 bg-coral-500/5"
                      : "border-zinc-800/70 bg-zinc-950/50")
                  }
                >
                  {isCapturing ? (
                    <input
                      autoFocus
                      readOnly
                      onKeyDown={(e) => handleKeyDown(b.id, e)}
                      onBlur={() => setCapturing(null)}
                      value="Pressione as teclas..."
                      className="w-full bg-transparent text-sm text-coral-300 outline-none"
                    />
                  ) : (
                    <KbdCombo keys={toKeys(combo)} />
                  )}
                  <Button
                    variant="secondary"
                    className="ml-3 shrink-0 px-4 py-1.5 text-xs"
                    disabled={saving}
                    onClick={() => {
                      setError("");
                      setCapturing(isCapturing ? null : b.id);
                    }}
                  >
                    {isCapturing ? "Cancelar" : "Alterar"}
                  </Button>
                </div>
              </div>
            </Card>
          );
        })}
      </div>
    </div>
  );
}
