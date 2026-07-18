import type { InputHTMLAttributes, SelectHTMLAttributes, TextareaHTMLAttributes } from "react";

const base =
  "w-full rounded-xl border border-zinc-800/70 bg-zinc-950/60 px-4 py-2.5 text-sm text-zinc-100 " +
  "placeholder:text-zinc-600 focus:outline-none focus:ring-2 focus:ring-coral-500/40 focus:border-coral-500/40 " +
  "transition-all duration-200";

export function Input(props: InputHTMLAttributes<HTMLInputElement>) {
  return <input {...props} className={base + " " + (props.className ?? "")} />;
}

export function Select(props: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      {...props}
      className={base + " cursor-pointer appearance-none " + (props.className ?? "")}
    />
  );
}

export function Textarea(props: TextareaAttributes) {
  return (
    <textarea
      {...props}
      className={base + " resize-none leading-relaxed " + (props.className ?? "")}
    />
  );
}

type TextareaAttributes = TextareaHTMLAttributes<HTMLTextAreaElement>;
