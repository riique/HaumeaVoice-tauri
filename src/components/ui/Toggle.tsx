export function Toggle({
  checked,
  onChange,
  disabled = false,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-disabled={disabled}
      disabled={disabled}
      onClick={() => {
        if (!disabled) onChange(!checked);
      }}
      className={
        "relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors duration-200 " +
        (disabled ? "opacity-40 cursor-not-allowed " : "") +
        (checked ? "bg-coral-500" : "bg-zinc-700")
      }
    >
      <span
        className={
          "inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform duration-200 " +
          (checked ? "translate-x-6" : "translate-x-1")
        }
      />
    </button>
  );
}

export function Checkbox({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
}) {
  return (
    <label className="inline-flex cursor-pointer items-center gap-2.5 text-sm text-zinc-300">
      <span
        onClick={() => onChange(!checked)}
        className={
          "flex h-5 w-5 items-center justify-center rounded-md border transition-all duration-200 " +
          (checked
            ? "bg-coral-500 border-coral-500 text-white"
            : "bg-zinc-900 border-zinc-700 hover:border-zinc-600")
        }
      >
        {checked && (
          <svg viewBox="0 0 16 16" className="h-3 w-3 fill-none stroke-current stroke-[3]">
            <path d="M3 8l3.5 3.5L13 5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        )}
      </span>
      {label}
    </label>
  );
}
