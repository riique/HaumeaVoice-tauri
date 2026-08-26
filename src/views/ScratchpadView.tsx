import { useEffect, useState } from "react";
import { Clipboard, Trash2 } from "lucide-react";
import { deleteScratchpadNote, getScratchpadNotes, type ScratchpadNote } from "../lib/tauri";
import { Button } from "../components/ui/Button";
import { EmptyState, PageHeader } from "../components/ui/Surface";

export function ScratchpadView() {
  const [notes, setNotes] = useState<ScratchpadNote[]>([]);
  const [loading, setLoading] = useState(true);
  useEffect(() => { getScratchpadNotes().then(setNotes).finally(() => setLoading(false)); }, []);
  const remove = async (id: string) => { if (await deleteScratchpadNote(id)) setNotes((items) => items.filter((item) => item.id !== id)); };
  return (
    <div className="space-y-8">
      <PageHeader title="Scratchpad" description="Notas rápidas ditadas sem colar no aplicativo em foco." />
      <div className="overflow-hidden rounded-[12px] border border-line bg-white">
        {!loading && notes.length === 0 ? <EmptyState title="Nenhuma nota rápida" description="Escolha Scratchpad como destino e dite normalmente pelo gadget." /> : (
          <div className="divide-y divide-line">
            {notes.map((note) => (
              <article key={note.id} className="px-5 py-4">
                <div className="flex items-start justify-between gap-6">
                  <div className="min-w-0">
                    <time className="text-[11px] tabular-nums text-muted">{new Date(note.created_at_ms).toLocaleString("pt-BR")}</time>
                    <p className="mt-2 whitespace-pre-wrap text-[14px] leading-6 text-ink">{note.text}</p>
                  </div>
                  <div className="flex shrink-0 gap-1">
                    <Button size="sm" variant="ghost" onClick={() => void navigator.clipboard.writeText(note.text)} aria-label="Copiar nota"><Clipboard className="h-4 w-4" aria-hidden /></Button>
                    <Button size="sm" variant="danger" onClick={() => void remove(note.id)} aria-label="Excluir nota"><Trash2 className="h-4 w-4" aria-hidden /></Button>
                  </div>
                </div>
              </article>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
