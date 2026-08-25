import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import {
  Home,
  Mic,
  History,
  Keyboard,
  Settings,
  type LucideIcon,
} from "lucide-react";
import type { ViewKey } from "../views";

const NAV: { key: ViewKey; label: string; icon: LucideIcon }[] = [
  { key: "inicio", label: "Início", icon: Home },
  { key: "transcricao", label: "Transcrição", icon: Mic },
  { key: "historico", label: "Histórico", icon: History },
  { key: "atalhos", label: "Atalhos", icon: Keyboard },
  { key: "configuracoes", label: "Configurações", icon: Settings },
];

export function Sidebar({
  current,
  onSelect,
}: {
  current: ViewKey;
  onSelect: (v: ViewKey) => void;
}) {
  const [version, setVersion] = useState("");

  useEffect(() => {
    getVersion().then(setVersion).catch(() => setVersion(""));
  }, []);

  return (
    <aside className="flex w-64 shrink-0 flex-col border-r border-zinc-800/50 bg-zinc-900">
      <div className="px-7 pt-9 pb-12">
        <div className="flex items-center gap-2.5">
          <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-coral-500 shadow-glow-coral">
            <Mic className="h-5 w-5 text-white" />
          </div>
          <div className="flex flex-col leading-tight">
            <span className="text-[15px] font-semibold tracking-tight text-zinc-100">
              Haumea Voice
            </span>
            <span className="text-[10px] uppercase tracking-[0.2em] text-zinc-500">
              Desktop
            </span>
          </div>
        </div>
      </div>

      <nav className="flex flex-1 flex-col gap-1.5 px-4">
        {NAV.map((item) => {
          const Icon = item.icon;
          const active = current === item.key;
          return (
            <button
              key={item.key}
              onClick={() => onSelect(item.key)}
              className={
                "group flex items-center gap-3 rounded-xl px-4 py-3 text-sm font-medium transition-all duration-200 " +
                (active
                  ? "bg-coral-500/15 text-coral-300"
                  : "text-zinc-400 hover:bg-zinc-800/60 hover:text-zinc-200")
              }
            >
              <Icon className="h-[18px] w-[18px]" />
              {item.label}
              {active && (
                <span className="ml-auto h-1.5 w-1.5 rounded-full bg-coral-500" />
              )}
            </button>
          );
        })}
      </nav>

      {version && (
        <div className="px-7 py-7 text-[11px] text-zinc-600">v{version}</div>
      )}
    </aside>
  );
}
