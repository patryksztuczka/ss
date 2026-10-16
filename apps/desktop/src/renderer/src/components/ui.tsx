import { useCallback, useRef, useState, type ButtonHTMLAttributes, type ReactNode } from "react";

export function cx(...classes: Array<string | false | null | undefined>) {
  return classes.filter(Boolean).join(" ");
}

export function Kbd({
  children,
  className,
}: {
  children: ReactNode;
  className?: string | undefined;
}) {
  return (
    <kbd className={cx("font-sans text-[11px] tracking-wide text-muted tabular-nums", className)}>
      {children}
    </kbd>
  );
}

export function Button({
  children,
  shortcut,
  variant = "ghost",
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { shortcut?: string; variant?: "ghost" | "primary" }) {
  return (
    <button
      type="button"
      {...props}
      className={cx(
        "no-drag inline-flex h-8 items-center gap-2 rounded-lg px-3 text-[13px] font-medium transition-all duration-200 ease-out-soft",
        "active:scale-[0.97] disabled:opacity-40",
        variant === "primary"
          ? "bg-ink text-white shadow-sm hover:bg-ink/90"
          : "bg-white text-ink shadow-[0_0_0_0.5px_rgb(0_0_0/0.1),0_1px_2px_rgb(0_0_0/0.05)] hover:bg-white/70",
        className,
      )}
    >
      {children}
      {shortcut !== undefined && (
        <Kbd className={variant === "primary" ? "text-white/60" : undefined}>{shortcut}</Kbd>
      )}
    </button>
  );
}

export function Slider({
  label,
  value,
  onChange,
  hint,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
  hint?: ReactNode;
}) {
  return (
    <label className="block">
      <div className="mb-1.5 flex items-baseline justify-between">
        <span className="text-[13px] font-medium text-ink">{label}</span>
        {hint ?? <span className="text-[11px] tabular-nums text-muted">{Math.round(value)}</span>}
      </div>
      <input
        className="slider"
        type="range"
        min={0}
        max={100}
        step={1}
        value={value}
        style={{ "--fill": `${value}%` } as React.CSSProperties}
        onChange={(event) => onChange(Number(event.currentTarget.value))}
      />
    </label>
  );
}

export function Segmented<T extends string>({
  options,
  value,
  onChange,
}: {
  options: ReadonlyArray<{ value: T; label: string }>;
  value: T;
  onChange: (value: T) => void;
}) {
  return (
    <div className="flex rounded-lg bg-black/[0.045] p-0.5">
      {options.map((option) => (
        <button
          type="button"
          key={option.value}
          onClick={() => onChange(option.value)}
          className={cx(
            "h-7 flex-1 rounded-[7px] text-[12px] font-medium transition-all duration-200 ease-out-soft",
            option.value === value
              ? "bg-white text-ink shadow-[0_0_0_0.5px_rgb(0_0_0/0.08),0_1px_2px_rgb(0_0_0/0.08)]"
              : "text-ink-soft hover:text-ink",
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function Section({
  title,
  children,
  aside,
}: {
  title: string;
  children: ReactNode;
  aside?: ReactNode;
}) {
  return (
    <section className="space-y-2.5">
      <div className="flex items-center justify-between">
        <h3 className="text-[11px] font-semibold tracking-[0.06em] text-muted uppercase">
          {title}
        </h3>
        {aside}
      </div>
      {children}
    </section>
  );
}

interface Toast {
  readonly id: number;
  readonly message: string;
}

/** Tiny transient status message ("Copied", "Saved to …"). */
export function useToast() {
  const [toast, setToast] = useState<Toast | null>(null);
  const counter = useRef(0);
  const show = useCallback((message: string) => {
    const id = ++counter.current;
    setToast({ id, message });
    window.setTimeout(() => setToast((current) => (current?.id === id ? null : current)), 2200);
  }, []);
  const node =
    toast === null ? null : (
      <div
        key={toast.id}
        className="pointer-events-none fixed bottom-20 left-1/2 z-50 -translate-x-1/2 animate-toast rounded-full bg-ink/90 px-4 py-2 text-[12px] font-medium text-white shadow-lg backdrop-blur"
      >
        {toast.message}
      </div>
    );
  return { show, node };
}

export function Switch({
  checked,
  onChange,
  disabled,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cx(
        "relative h-[22px] w-[38px] shrink-0 rounded-full transition-colors duration-300 ease-out-soft disabled:opacity-40",
        checked ? "bg-accent" : "bg-black/[0.12]",
      )}
    >
      <span
        className={cx(
          "absolute top-[2px] left-[2px] size-[18px] rounded-full bg-white shadow-[0_1px_3px_rgb(0_0_0/0.25)] transition-transform duration-300 ease-spring",
          checked && "translate-x-4",
        )}
      />
    </button>
  );
}

/** Individual key caps, e.g. [⇧][⌘][4]. */
export function KeyCaps({
  keys,
  size = "md",
}: {
  keys: ReadonlyArray<string>;
  size?: "md" | "lg";
}) {
  return (
    <span className="inline-flex items-center gap-1">
      {keys.map((key, index) => (
        <kbd
          key={`${key}-${index}`}
          className={cx(
            "grid place-items-center rounded-md bg-white font-sans font-medium text-ink tabular-nums",
            "shadow-[0_0_0_0.5px_rgb(0_0_0/0.14),0_1.5px_0_rgb(0_0_0/0.08)]",
            size === "lg" ? "h-9 min-w-9 px-2 text-[16px]" : "h-6 min-w-6 px-1.5 text-[12px]",
          )}
        >
          {key}
        </kbd>
      ))}
    </span>
  );
}

export function StatusPill({ ok, children }: { ok: boolean; children: ReactNode }) {
  return (
    <span
      className={cx(
        "inline-flex h-7 items-center gap-1.5 rounded-full px-2.5 text-[12px] font-medium animate-fade-in",
        ok ? "bg-emerald-500/10 text-emerald-700" : "bg-amber-500/12 text-amber-700",
      )}
    >
      {ok ? (
        <svg
          viewBox="0 0 12 12"
          className="size-3"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
        >
          <path d="M2.5 6.3 5 8.6l4.5-5.2" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      ) : (
        <span className="size-1.5 rounded-full bg-current" />
      )}
      {children}
    </span>
  );
}

export const card =
  "rounded-xl bg-white shadow-[0_0_0_0.5px_rgb(0_0_0/0.08),0_1px_2px_rgb(0_0_0/0.03)]";
