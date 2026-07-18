import type { ReactNode } from "react";

export function Kbd({ children }: { children: ReactNode }) {
  return (
    <span className="kbd font-mono">
      {children}
    </span>
  );
}

export function KbdCombo({ keys }: { keys: string[] }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      {keys.map((k, i) => (
        <span key={k} className="inline-flex items-center gap-1.5">
          <Kbd>{k}</Kbd>
          {i < keys.length - 1 && (
            <span className="text-zinc-600 text-xs">+</span>
          )}
        </span>
      ))}
    </span>
  );
}
