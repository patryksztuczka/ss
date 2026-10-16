import { Effect } from "effect";
import { useCallback, useEffect, useState, type ReactNode } from "react";

import iconUrl from "../../../../resources/icon.png";
import type { SetupState } from "../../../shared/ipc";
import {
  shortcutPresets,
  type Settings,
  type ShortcutAction,
  type ShortcutPreset,
} from "../../../shared/settings";
import { Button, KeyCaps, StatusPill, Switch, card, cx } from "../components/ui";
import { acceleratorParts } from "../lib/accelerator";
import { run, useEffectState } from "../lib/hooks";
import { invoke } from "../lib/ipc";

const STEPS = ["welcome", "permissions", "shortcuts", "done"] as const;
type Step = (typeof STEPS)[number];

const ACTION_LABELS: ReadonlyArray<{ action: ShortcutAction; label: string }> = [
  { action: "captureArea", label: "Capture area or window" },
  { action: "captureScreen", label: "Capture screen" },
  { action: "recordArea", label: "Record area" },
  { action: "recordScreen", label: "Record screen" },
  { action: "openLast", label: "Edit last capture" },
];

export function Onboarding() {
  const state = useEffectState(
    () => Effect.all([invoke("setup:state"), invoke("settings:get")], { concurrency: "unbounded" }),
    [],
  );
  if (state._tag !== "Success") return <div className="drag h-full bg-canvas" />;
  return <OnboardingView initialSetup={state.value[0]} initialSettings={state.value[1]} />;
}

function OnboardingView({
  initialSetup,
  initialSettings,
}: {
  initialSetup: SetupState;
  initialSettings: Settings;
}) {
  const [step, setStep] = useState<Step>("welcome");
  const [setup, setSetup] = useState(initialSetup);
  const [settings, setSettings] = useState(initialSettings);
  const [preset, setPreset] = useState<ShortcutPreset>(
    initialSettings.shortcuts.captureArea === shortcutPresets.alongside.captureArea &&
      initialSettings.onboarded
      ? "alongside"
      : "system",
  );
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState<ReadonlyArray<ShortcutAction>>([]);
  const index = STEPS.indexOf(step);

  const next = useCallback(() => {
    if (step === "shortcuts") {
      setBusy(true);
      return run(
        invoke("setup:apply-shortcuts", { preset }).pipe(
          Effect.tap((result) =>
            Effect.sync(() => {
              setSettings(result.settings);
              setFailed(result.failedShortcuts);
              if (result.failedShortcuts.length === 0) setStep("done");
            }),
          ),
          Effect.andThen(invoke("setup:state")),
          Effect.tap((value) => Effect.sync(() => setSetup(value))),
          Effect.ensuring(Effect.sync(() => setBusy(false))),
        ),
      );
    }
    if (step === "done") return run(invoke("setup:finish"));
    setStep(STEPS[index + 1] ?? "done");
  }, [index, preset, step]);

  const back = () => setStep(STEPS[Math.max(0, index - 1)] ?? "welcome");

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Enter" && !busy) next();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [busy, next]);

  const primaryLabel =
    step === "welcome"
      ? "Get Started"
      : step === "permissions" && setup.screenAccess !== "granted"
        ? "Skip for Now"
        : step === "shortcuts"
          ? "Use These Shortcuts"
          : step === "done"
            ? "Start Using ss"
            : "Continue";

  return (
    <div className="flex h-full flex-col bg-canvas">
      <header className="drag flex h-12 shrink-0 items-center justify-center gap-1.5">
        {STEPS.map((name, i) => (
          <span
            key={name}
            className={cx(
              "h-1.5 rounded-full transition-all duration-500 ease-out-soft",
              i === index ? "w-5 bg-ink" : i < index ? "w-1.5 bg-ink/40" : "w-1.5 bg-ink/12",
            )}
          />
        ))}
      </header>

      <main key={step} className="flex min-h-0 flex-1 flex-col items-center px-14 animate-rise-in">
        {step === "welcome" && <Welcome />}
        {step === "permissions" && <Permissions setup={setup} onChange={setSetup} />}
        {step === "shortcuts" && (
          <ShortcutsStep preset={preset} onPreset={setPreset} failed={failed} />
        )}
        {step === "done" && <Done setup={setup} settings={settings} onSetup={setSetup} />}
      </main>

      <footer className="flex h-[72px] shrink-0 items-center justify-between px-8">
        <div>
          {index > 0 && step !== "done" && (
            <button
              type="button"
              onClick={back}
              className="text-[13px] font-medium text-ink-soft transition-colors hover:text-ink"
            >
              Back
            </button>
          )}
        </div>
        <Button variant="primary" shortcut="↩" disabled={busy} onClick={next} className="h-9 px-4">
          {primaryLabel}
        </Button>
      </footer>
    </div>
  );
}

function StepTitle({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="mb-7 text-center">
      <h1 className="text-[22px] font-semibold tracking-[-0.01em]">{title}</h1>
      <p className="mx-auto mt-1.5 max-w-[440px] text-[13.5px] leading-relaxed text-ink-soft">
        {children}
      </p>
    </div>
  );
}

function Welcome() {
  const features = [
    {
      title: "Clipboard first",
      text: "Every capture is ready to paste. Nothing lands on your Desktop.",
    },
    {
      title: "Nice backgrounds",
      text: "Turn a raw screenshot into something worth sharing in a second.",
    },
    { title: "Selectable text", text: "Copy words straight out of any screenshot, on-device." },
  ];
  return (
    <div className="flex flex-1 flex-col items-center justify-center pb-6">
      <img
        src={iconUrl}
        alt=""
        className="mb-5 size-[104px] drop-shadow-[0_12px_24px_rgb(91_90_220/0.28)] animate-[rise-in_700ms_var(--ease-spring)_both]"
      />
      <StepTitle title="Welcome to ss">
        Screenshots and recordings that go straight to your clipboard. Let's set things up — it
        takes less than a minute.
      </StepTitle>
      <div className="grid w-full max-w-[600px] grid-cols-3 gap-3">
        {features.map((feature, i) => (
          <div
            key={feature.title}
            className={cx(card, "p-4 animate-rise-in")}
            style={{ animationDelay: `${120 + i * 70}ms` }}
          >
            <div className="text-[13px] font-semibold">{feature.title}</div>
            <div className="mt-1 text-[12px] leading-relaxed text-ink-soft">{feature.text}</div>
          </div>
        ))}
      </div>
    </div>
  );
}

function Permissions({
  setup,
  onChange,
}: {
  setup: SetupState;
  onChange: (state: SetupState) => void;
}) {
  const [requested, setRequested] = useState(false);
  const granted = setup.screenAccess === "granted";

  // After asking, watch for the toggle in System Settings (a fresh check runs each time).
  useEffect(() => {
    if (!requested || granted) return;
    const timer = window.setInterval(
      () =>
        run(invoke("setup:state").pipe(Effect.tap((value) => Effect.sync(() => onChange(value))))),
      1500,
    );
    return () => window.clearInterval(timer);
  }, [requested, granted, onChange]);

  const request = () => {
    setRequested(true);
    run(
      invoke("setup:request-screen-access").pipe(
        Effect.tap((value) => Effect.sync(() => onChange(value))),
      ),
    );
  };

  const appName = setup.permissionHost ?? "ss";

  return (
    <div className="flex w-full max-w-[560px] flex-1 flex-col justify-center pb-8">
      <StepTitle title="One permission">
        ss needs to see your screen to capture it. Everything stays on your Mac.
      </StepTitle>

      <div className={cx(card, "flex items-center gap-4 p-4")}>
        <div className="grid size-11 shrink-0 place-items-center rounded-[11px] bg-[linear-gradient(135deg,#8fe6f8,#9a7cf2)] text-white shadow-sm">
          <svg
            viewBox="0 0 24 24"
            className="size-6"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.7"
          >
            <rect x="3" y="4.5" width="18" height="12" rx="2" />
            <path d="M8.5 20h7M12 16.5V20" strokeLinecap="round" />
            <circle cx="12" cy="10.5" r="2.2" fill="currentColor" stroke="none" />
          </svg>
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-[14px] font-semibold">Screen Recording</div>
          <div className="text-[12.5px] leading-snug text-ink-soft">
            Required for screenshots and recordings.
          </div>
        </div>
        {granted ? (
          <StatusPill ok>Granted</StatusPill>
        ) : (
          <Button variant="primary" onClick={request} className="h-8">
            {requested ? "Open Settings" : "Grant Access"}
          </Button>
        )}
      </div>

      <div className="mt-4 min-h-[60px] text-center text-[12.5px] leading-relaxed text-ink-soft">
        {granted ? (
          <span className="animate-fade-in">You're good to go.</span>
        ) : requested ? (
          <span className="animate-fade-in">
            In System Settings, switch on <b className="font-semibold text-ink">{appName}</b> under
            Screen &amp; System Audio Recording. This page updates by itself.
          </span>
        ) : setup.permissionHost !== null ? (
          <span>
            Running in development: macOS asks{" "}
            <b className="font-semibold text-ink">{setup.permissionHost}</b> (the app that started
            ss) for this permission.
          </span>
        ) : null}
      </div>
    </div>
  );
}

function ShortcutsStep({
  preset,
  onPreset,
  failed,
}: {
  preset: ShortcutPreset;
  onPreset: (preset: ShortcutPreset) => void;
  failed: ReadonlyArray<ShortcutAction>;
}) {
  const options: ReadonlyArray<{
    value: ShortcutPreset;
    title: string;
    text: string;
    badge?: string;
  }> = [
    {
      value: "system",
      title: "Replace macOS shortcuts",
      text: "ss takes over the keys you already know. The macOS screenshot shortcuts are switched off — you can bring them back in Settings.",
      badge: "Recommended",
    },
    {
      value: "alongside",
      title: "Keep macOS shortcuts",
      text: "ss uses its own keys, and the built-in screenshot tool keeps working as before.",
    },
  ];
  const keys = shortcutPresets[preset];

  return (
    <div className="flex w-full max-w-[620px] flex-1 flex-col justify-center pb-4">
      <StepTitle title="Your shortcuts">Pick the keys you'll press every day.</StepTitle>

      <div className="grid grid-cols-2 gap-3">
        {options.map((option) => {
          const selected = option.value === preset;
          const sample = shortcutPresets[option.value];
          return (
            <button
              type="button"
              key={option.value}
              onClick={() => onPreset(option.value)}
              className={cx(
                card,
                "relative flex flex-col justify-start p-4 text-left transition-all duration-300 ease-out-soft active:scale-[0.99]",
                selected
                  ? "ring-2 ring-accent"
                  : "hover:shadow-[0_0_0_0.5px_rgb(0_0_0/0.14),0_4px_12px_rgb(0_0_0/0.06)]",
              )}
            >
              <div className="flex items-center justify-between">
                <span className="text-[13.5px] font-semibold">{option.title}</span>
                {option.badge !== undefined && (
                  <span className="rounded-full bg-accent-soft px-2 py-0.5 text-[10.5px] font-semibold text-accent">
                    {option.badge}
                  </span>
                )}
              </div>
              <div className="mt-3 flex gap-2">
                {[sample.captureScreen, sample.captureArea, sample.recordArea].map(
                  (accelerator) => (
                    <KeyCaps key={accelerator} keys={acceleratorParts(accelerator)} />
                  ),
                )}
              </div>
              <p className="mt-3 text-[12px] leading-relaxed text-ink-soft">{option.text}</p>
            </button>
          );
        })}
      </div>

      <div className={cx(card, "mt-4 divide-y divide-line")}>
        {ACTION_LABELS.map(({ action, label }) => (
          <div key={action} className="flex h-9 items-center justify-between px-4">
            <span
              className={cx("text-[12.5px]", failed.includes(action) ? "text-red-500" : "text-ink")}
            >
              {label}
              {failed.includes(action) && " — taken by another app"}
            </span>
            <KeyCaps keys={acceleratorParts(keys[action])} />
          </div>
        ))}
      </div>
    </div>
  );
}

function Done({
  setup,
  settings,
  onSetup,
}: {
  setup: SetupState;
  settings: Settings;
  onSetup: (state: SetupState) => void;
}) {
  const toggleLogin = (enabled: boolean) =>
    run(
      invoke("setup:launch-at-login", { enabled }).pipe(
        Effect.tap((value) => Effect.sync(() => onSetup(value))),
      ),
    );

  return (
    <div className="flex w-full max-w-[520px] flex-1 flex-col items-center justify-center pb-6">
      <div className="mb-5 grid size-16 place-items-center rounded-full bg-emerald-500/12 text-emerald-600 animate-[rise-in_600ms_var(--ease-spring)_both]">
        <svg
          viewBox="0 0 24 24"
          className="size-8"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.2"
        >
          <path d="m5.5 12.5 4.2 4 8.8-9.5" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </div>
      <StepTitle title="You're all set">
        ss lives in your menu bar. Try it now — press the shortcut below.
      </StepTitle>

      <div className="mb-7 flex flex-col items-center gap-2">
        <KeyCaps size="lg" keys={acceleratorParts(settings.shortcuts.captureArea)} />
        <span className="text-[12px] text-muted">Capture an area · press Space for a window</span>
      </div>

      <div className={cx(card, "flex w-full items-center justify-between gap-4 px-4 py-3")}>
        <div>
          <div className="text-[13px] font-medium">Launch ss at login</div>
          <div className="text-[11.5px] text-muted">
            {setup.launchAtLogin.available
              ? "So your shortcuts always work."
              : "Available in the installed app."}
          </div>
        </div>
        <Switch
          checked={setup.launchAtLogin.enabled}
          disabled={!setup.launchAtLogin.available}
          onChange={toggleLogin}
        />
      </div>
    </div>
  );
}
