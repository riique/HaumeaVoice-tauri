import { useEffect, useState } from "react";
import { Cloud, UploadCloud, FileAudio, Loader2, CheckCircle2, AlertCircle } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Button } from "../components/ui/Button";
import { Card } from "../components/ui/Card";
import { Select } from "../components/ui/Input";
import { transcribeFile } from "../lib/tauri";

type Status = "idle" | "transcribing" | "done" | "error";

const AUDIO_EXTENSIONS = ["wav", "mp3", "m4a", "flac", "ogg", "aac", "webm", "mp4"];

/** Extracts the file name from an absolute path (handles both separators). */
function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function TranscricaoView() {
  const [filePath, setFilePath] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [status, setStatus] = useState<Status>("idle");
  const [result, setResult] = useState("");
  const [error, setError] = useState("");

  // Native (Tauri) drag-and-drop. The webview reports file paths directly, so
  // we keep only the first dropped file and ignore non-audio extensions.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    (async () => {
      unlisten = await getCurrentWebview().onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === "over" || p.type === "enter") {
          setDragging(true);
        } else if (p.type === "leave") {
          setDragging(false);
        } else if (p.type === "drop") {
          setDragging(false);
          const dropped = p.paths?.[0];
          if (dropped && AUDIO_EXTENSIONS.includes(dropped.split(".").pop()?.toLowerCase() ?? "")) {
            selectPath(dropped);
          }
        }
      });
    })();
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  const selectPath = (p: string) => {
    setFilePath(p);
    setStatus("idle");
    setResult("");
    setError("");
  };

  const handleBrowse = async () => {
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "Áudio", extensions: AUDIO_EXTENSIONS }],
      });
      if (typeof selected === "string") {
        selectPath(selected);
      }
    } catch (e) {
      console.error("file dialog failed:", e);
    }
  };

  const handleTranscribe = async () => {
    if (!filePath) return;
    setStatus("transcribing");
    setError("");
    setResult("");
    try {
      const text = await transcribeFile(filePath);
      setResult(text);
      setStatus("done");
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
      setStatus("error");
    }
  };

  return (
    <div className="space-y-8">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight text-zinc-100">
          Transcrição
        </h1>
        <p className="mt-1 text-sm text-zinc-500">
          Envie um arquivo de áudio para transcrever via motores em nuvem.
        </p>
      </header>

      {/* Drag & Drop gigante */}
      <Card
        onClick={handleBrowse}
        className={
          "flex min-h-[280px] cursor-pointer flex-col items-center justify-center gap-4 p-12 text-center transition-all duration-300 " +
          (dragging || filePath
            ? "border-coral-500/50 bg-coral-500/5"
            : "border-dashed border-zinc-700 hover:border-coral-500/40 hover:bg-zinc-800/20")
        }
      >
        <div className="flex flex-col items-center gap-4">
          <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-zinc-800 text-coral-400">
            {filePath ? <FileAudio className="h-8 w-8" /> : <UploadCloud className="h-8 w-8" />}
          </div>
          {filePath ? (
            <>
              <p className="text-base font-medium text-zinc-200">{baseName(filePath)}</p>
              <p className="text-xs text-zinc-600">Clique para escolher outro arquivo</p>
            </>
          ) : (
            <>
              <p className="text-base font-medium text-zinc-200">
                Clique ou arraste seu arquivo de áudio aqui para transcrever
              </p>
              <p className="text-xs text-zinc-600">
                Formatos suportados: WAV, MP3, M4A, FLAC
              </p>
            </>
          )}
        </div>
      </Card>

      {/* Seletores em grid */}
      <div className="grid grid-cols-2 gap-5">
        <div className="space-y-2">
          <label className="flex items-center gap-2 text-xs font-medium uppercase tracking-wider text-zinc-500">
            <Cloud className="h-3.5 w-3.5" /> Modelo (Nuvem)
          </label>
          <Select defaultValue="ativo" disabled>
            <option value="ativo">Usa o motor ativo (Ajustes › Motores)</option>
          </Select>
        </div>

        <div className="space-y-2">
          <label className="text-xs font-medium uppercase tracking-wider text-zinc-500">
            Idioma
          </label>
          <Select defaultValue="auto">
            <option value="auto">Detectar Automaticamente</option>
            <option value="pt-BR">Português (Brasil)</option>
            <option value="en-US">Inglês (EUA)</option>
            <option value="es-ES">Espanhol</option>
          </Select>
        </div>
      </div>

      {/* Botão de ação */}
      <div className="flex items-center justify-end gap-3 pt-2">
        {status === "error" && (
          <span className="flex items-center gap-1.5 text-sm text-red-400">
            <AlertCircle className="h-4 w-4" /> {error}
          </span>
        )}
        {status === "done" && (
          <span className="flex items-center gap-1.5 text-sm text-emerald-400">
            <CheckCircle2 className="h-4 w-4" /> Transcrição salva no histórico.
          </span>
        )}
        <Button
          variant="primary"
          disabled={!filePath || status === "transcribing"}
          className="gap-2 px-8 py-3 text-base"
          onClick={handleTranscribe}
        >
          {status === "transcribing" && <Loader2 className="h-4 w-4 animate-spin" />}
          {status === "transcribing" ? "Transcrevendo..." : "Transcrever Arquivo"}
        </Button>
      </div>

      {/* Resultado */}
      {result && (
        <Card className="p-7">
          <h3 className="mb-3 text-xs font-medium uppercase tracking-wider text-zinc-500">
            Resultado
          </h3>
          <p className="whitespace-pre-wrap text-[15px] leading-relaxed text-zinc-300">
            {result}
          </p>
        </Card>
      )}
    </div>
  );
}
