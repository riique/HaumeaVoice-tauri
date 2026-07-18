import type { ButtonHTMLAttributes, ReactNode } from "react";

type Variant = "primary" | "secondary" | "ghost" | "danger";

const VARIANTS: Record<Variant, string> = {
  primary:
    "bg-coral-500 hover:bg-coral-600 text-white shadow-[0_4px_14px_-2px_rgba(225,77,42,0.5)] hover:shadow-glow-coral",
  secondary:
    "bg-zinc-800 hover:bg-zinc-700 text-zinc-100 border border-zinc-700/60",
  ghost: "bg-transparent hover:bg-zinc-800/60 text-zinc-300",
  danger:
    "bg-transparent hover:bg-red-500/10 text-red-400 hover:text-red-300 border border-zinc-800",
};

export function Button({
  children,
  variant = "secondary",
  className = "",
  ...props
}: {
  children: ReactNode;
  variant?: Variant;
} & ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      className={
        "inline-flex items-center justify-center gap-2 rounded-xl px-4 py-2.5 text-sm font-medium " +
        "transition-all duration-200 focus:outline-none focus:ring-2 focus:ring-coral-500/40 disabled:opacity-40 " +
        "disabled:cursor-not-allowed disabled:hover:shadow-none " +
        VARIANTS[variant] +
        " " +
        className
      }
      {...props}
    >
      {children}
    </button>
  );
}
