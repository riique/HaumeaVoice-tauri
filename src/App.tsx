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
    <div className="relative flex h-screen w-screen overflow-hidden bg-canvas text-ink">
      <a href="#main-content" className="skip-link">Pular para o conteúdo</a>
      <TitleBar />

      <Sidebar current={view} onSelect={setView} />

      <main id="main-content" tabIndex={-1} className="scrollbar-thin min-w-0 flex-1 overflow-y-auto">
        <div className="page-shell">
          {view === "inicio" && <InicioView onNavigate={setView} />}
          {view === "transcricao" && <TranscricaoView />}
          {view === "historico" && <HistoricoView />}
          {view === "atalhos" && <AtalhosView />}
          {view === "configuracoes" && <ConfiguracoesView />}
        </div>
      </main>
    </div>
  );
}
