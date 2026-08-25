import { Check } from "lucide-react";

export function Toggle({
  checked,
  onChange,
  label,
  disabled = false,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-label={label}
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={
        "relative inline-flex h-6 w-10 shrink-0 items-center rounded-full transition-colors duration-150 disabled:cursor-not-allowed disabled:opacity-40 " +
        (checked ? "bg-[#242422]" : "bg-[#c9c9c2]")
      }
    >
      <span
        className={
          "h-4 w-4 rounded-full bg-white shadow-[0_1px_3px_rgba(0,0,0,.24)] transition-transform duration-150 " +
          (checked ? "translate-x-5" : "translate-x-1")
        }
      />
    </button>
  );
}

export function Checkbox({
  checked,
  onChange,
  label,
  disabled = false,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <label className="inline-flex cursor-pointer items-center gap-2 text-[13px] text-[#4d4e49] has-[:disabled]:cursor-not-allowed has-[:disabled]:opacity-50">
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
        className="peer sr-only"
      />
      <span className="flex h-[18px] w-[18px] items-center justify-center rounded-[5px] border border-[#c9c9c2] bg-white transition-colors peer-checked:border-[#242422] peer-checked:bg-[#242422] peer-focus-visible:outline peer-focus-visible:outline-2 peer-focus-visible:outline-offset-2 peer-focus-visible:outline-[#242422]">
        {checked && <Check className="h-3 w-3 text-white" strokeWidth={2.6} aria-hidden />}
      </span>
      {label}
    </label>
  );
}
