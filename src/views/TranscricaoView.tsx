import { useEffect, useState } from "react";
import { AlertCircle, CheckCircle2, FileAudio, Loader2, UploadCloud } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Button } from "../components/ui/Button";
import { PageHeader } from "../components/ui/Surface";
import { transcribeFile } from "../lib/tauri";

type Status = "idle" | "transcribing" | "done" | "error";

const AUDIO_EXTENSIONS = ["wav", "mp3", "m4a", "flac", "ogg", "aac", "webm", "mp4"];

function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function TranscricaoView() {
  const [filePath, setFilePath] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [status, setStatus] = useState<Status>("idle");
  const [result, setResult] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload;
      if (payload.type === "over" || payload.type === "enter") {
        setDragging(true);
        return;
      }
      if (payload.type === "leave") {
        setDragging(false);
        return;
      }
      if (payload.type === "drop") {
        setDragging(false);
        const dropped = payload.paths?.[0];
        const extension = dropped?.split(".").pop()?.toLowerCase() ?? "";
        if (dropped && AUDIO_EXTENSIONS.includes(extension)) selectPath(dropped);
      }
    }).then((dispose) => {
      unlisten = dispose;
    });
    return () => unlisten?.();
  }, []);

  const selectPath = (path: string) => {
    setFilePath(path);
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
      if (typeof selected === "string") selectPath(selected);
    } catch (browseError) {
      console.error("file dialog failed:", browseError);
    }
  };

  const handleTranscribe = async () => {
    if (!filePath) return;
    setStatus("transcribing");
    setError("");
    setResult("");
    try {
      setResult(await transcribeFile(filePath));
      setStatus("done");
    } catch (transcriptionError) {
      setError(typeof transcriptionError === "string" ? transcriptionError : String(transcriptionError));
      setStatus("error");
    }
  };

  return (
    <div className="space-y-8">
      <PageHeader title="Transcrição" description="Envie um arquivo de áudio para transcrever com a pipeline ativa." />

      <section className="surface overflow-hidden" aria-labelledby="upload-title">
        <button
          type="button"
          onClick={handleBrowse}
          className={
            "m-6 flex min-h-[330px] w-[calc(100%-3rem)] flex-col items-center justify-center rounded-[12px] border border-dashed px-10 py-14 text-center transition-colors " +
            (dragging
              ? "border-[#656660] bg-[#f0f0eb]"
              : "border-[#d7d7d1] bg-[#fcfcfa] hover:border-[#a8a9a2] hover:bg-[#f8f8f4]")
          }
          aria-describedby="upload-formats"
        >
          <span className="flex h-12 w-12 items-center justify-center rounded-full border border-line bg-white text-[#50514c]">
            {filePath ? <FileAudio className="h-5 w-5" aria-hidden /> : <UploadCloud className="h-5 w-5" aria-hidden />}
          </span>
          <span id="upload-title" className="mt-5 text-[15px] font-medium text-ink">
            {filePath ? baseName(filePath) : "Clique ou arraste seu arquivo de áudio aqui"}
          </span>
          <span id="upload-formats" className="mt-2 text-[13px] text-muted">
            {filePath ? "Clique para escolher outro arquivo" : "WAV, MP3, M4A, FLAC, OGG, AAC, WEBM ou MP4"}
          </span>
          {!filePath && <span className="mt-5 text-[12px] font-medium text-[#555650]">Escolher arquivo</span>}
        </button>

        <div className="flex min-h-[62px] items-center justify-between gap-5 border-t border-line px-6 py-4">
          <p className="text-[12px] leading-5 text-muted">A pipeline definida em Configurações será usada automaticamente.</p>
          <Button variant="primary" disabled={!filePath || status === "transcribing"} onClick={handleTranscribe}>
            {status === "transcribing" && <Loader2 className="h-4 w-4 animate-spin" aria-hidden />}
            {status === "transcribing" ? "Transcrevendo…" : "Transcrever arquivo"}
          </Button>
        </div>
      </section>

      {status === "error" && (
        <div className="flex items-start gap-3 rounded-[10px] bg-[#fff1ef] px-4 py-3 text-[13px] text-[#9f2720]" role="alert">
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
          <span>{error}</span>
        </div>
      )}

      {status === "done" && (
        <div className="surface overflow-hidden">
          <div className="flex items-center gap-2 border-b border-line px-5 py-4 text-[13px] font-medium text-[#25613f]" role="status">
            <CheckCircle2 className="h-4 w-4" aria-hidden />
            Transcrição concluída e salva no histórico
          </div>
          <p className="whitespace-pre-wrap px-5 py-5 text-[14px] leading-6 text-[#343530]">{result}</p>
        </div>
      )}
    </div>
  );
}
