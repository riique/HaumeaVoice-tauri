import { useState } from "react";
import { Sidebar } from "./components/Sidebar";
import { TitleBar } from "./components/TitleBar";
import { InicioView } from "./views/InicioView";
import { TranscricaoView } from "./views/TranscricaoView";
import { HistoricoView } from "./views/HistoricoView";
import { AtalhosView } from "./views/AtalhosView";
import { ConfiguracoesView } from "./views/ConfiguracoesView";
import type { ViewKey } from "./views";

export default function App() {
  const [view, setView] = useState<ViewKey>("inicio");

  return (
    <div className="relative flex h-screen w-screen overflow-hidden bg-zinc-950 text-zinc-100">
      {/* Transparent frameless title bar overlaid on the top edge. It takes no
          vertical space, so the sidebar and main content keep their original
          top alignment (no downward shift). */}
      <TitleBar />

      <Sidebar current={view} onSelect={setView} />

      <main className="scrollbar-thin flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[1400px] px-14 py-12">
          {view === "inicio" && <InicioView />}
          {view === "transcricao" && <TranscricaoView />}
          {view === "historico" && <HistoricoView />}
          {view === "atalhos" && <AtalhosView />}
          {view === "configuracoes" && <ConfiguracoesView />}
        </div>
      </main>
    </div>
  );
}
