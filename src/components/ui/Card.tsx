import type { ReactNode } from "react";

type CardVariant = "default" | "hub";

const VARIANTS: Record<CardVariant, string> = {
  default: "rounded-xl shadow-elevated",
  hub: "rounded-2xl shadow-elevated-xl",
};

export function Card({
  children,
  className = "",
  variant = "default",
  onClick,
}: {
  children: ReactNode;
  className?: string;
  variant?: CardVariant;
  onClick?: () => void;
}) {
  return (
    <div
      onClick={onClick}
      className={
        "border border-zinc-800/50 bg-zinc-900 " +
        VARIANTS[variant] +
        " " +
        className
      }
    >
      {children}
    </div>
  );
}
