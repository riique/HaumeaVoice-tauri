import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import {
  AudioLines,
  FileAudio,
  History,
  LifeBuoy,
  Home,
  Keyboard,
  Settings,
  NotebookPen,
  ChartNoAxesCombined,
  type LucideIcon,
} from "lucide-react";
import type { ViewKey } from "../views";

const NAV: { key: ViewKey; label: string; icon: LucideIcon }[] = [
  { key: "inicio", label: "Início", icon: Home },
  { key: "transcricao", label: "Transcrição", icon: FileAudio },
  { key: "historico", label: "Histórico", icon: History },
  { key: "insights", label: "Insights", icon: ChartNoAxesCombined },
  { key: "scratchpad", label: "Scratchpad", icon: NotebookPen },
  { key: "recuperacao", label: "Recuperação", icon: LifeBuoy },
  { key: "atalhos", label: "Atalhos", icon: Keyboard },
  { key: "configuracoes", label: "Configurações", icon: Settings },
];

export function Sidebar({
  current,
  onSelect,
}: {
  current: ViewKey;
  onSelect: (view: ViewKey) => void;
}) {
  const [version, setVersion] = useState("");

  useEffect(() => {
    getVersion().then(setVersion).catch(() => setVersion(""));
  }, []);

  return (
    <aside className="flex w-[216px] shrink-0 flex-col border-r border-line bg-sidebar pb-6 pt-8 max-[1180px]:w-[76px]">
      <div className="flex h-12 items-center gap-2.5 px-5 max-[1180px]:justify-center max-[1180px]:px-0">
        <div className="flex h-8 w-8 items-center justify-center rounded-[10px] border border-[#cacbc4] bg-white text-ink">
          <AudioLines className="h-[17px] w-[17px]" strokeWidth={1.8} aria-hidden />
        </div>
        <span className="text-[17px] font-semibold tracking-[-0.02em] text-ink max-[1180px]:hidden">
          Haumea
        </span>
      </div>

      <nav className="mt-8 flex flex-1 flex-col gap-1 px-3" aria-label="Navegação principal">
        {NAV.map((item) => {
          const Icon = item.icon;
          const active = current === item.key;
          return (
            <button
              key={item.key}
              type="button"
              aria-current={active ? "page" : undefined}
              aria-label={item.label}
              title={item.label}
              onClick={() => onSelect(item.key)}
              className={
                "flex h-10 items-center gap-3 rounded-[10px] px-3 text-[13px] font-medium transition-colors duration-150 max-[1180px]:justify-center max-[1180px]:px-0 " +
                (active
                  ? "bg-[#dfdfd9] text-ink"
                  : "text-[#5e5f59] hover:bg-[#e8e8e3] hover:text-ink")
              }
            >
              <Icon className="h-[17px] w-[17px] shrink-0" strokeWidth={1.8} aria-hidden />
              <span className="max-[1180px]:hidden">{item.label}</span>
            </button>
          );
        })}
      </nav>

      {version && (
        <div className="px-5 text-[11px] tabular-nums text-[#5d5e58] max-[1180px]:px-0 max-[1180px]:text-center">
          <span className="max-[1180px]:hidden">Haumea Voice </span>v{version}
        </div>
      )}
    </aside>
  );
}
