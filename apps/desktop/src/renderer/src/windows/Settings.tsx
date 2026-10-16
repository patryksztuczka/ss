import { Effect } from "effect";
import { useEffect, useState } from "react";

import type { AppInfo, SetupState } from "../../../shared/ipc";
import {
  isUsableAccelerator,
  type AfterCapture,
  type Settings as SettingsValue,
  type ShortcutAction,
} from "../../../shared/settings";
import { Button, Section, Segmented, StatusPill, Switch, card, cx } from "../components/ui";
import { acceleratorFromEvent, formatAccelerator } from "../lib/accelerator";
import { run, useEffectState, useEvent } from "../lib/hooks";
import { invoke } from "../lib/ipc";

const SHORTCUTS: ReadonlyArray<{ action: ShortcutAction; label: string }> = [
  { action: "captureArea", label: "Capture area or window" },
  { action: "captureScreen", label: "Capture screen" },
  { action: "recordArea", label: "Record area · stop" },
  { action: "recordScreen", label: "Record screen · stop" },
  { action: "openLast", label: "Edit last capture" },
];

const AFTER_CAPTURE: ReadonlyArray<{ value: AfterCapture; label: string }> = [
  { value: "preview", label: "Show preview" },
  { value: "editor", label: "Open editor" },
  { value: "nothing", label: "Just copy" },
];

const tildify = (path: string) => path.replace(/^\/Users\/[^/]+/, "~");

export function Settings() {
  const state = useEffectState(
    () =>
      Effect.all([invoke("settings:get"), invoke("app:info"), invoke("setup:state")], {
        concurrency: "unbounded",
      }),
    [],
  );
  if (state._tag !== "Success") return <div className="drag h-full bg-canvas" />;
  return (
    <SettingsView initial={state.value[0]} info={state.value[1]} initialSetup={state.value[2]} />
  );
}

function SettingsView({
  initial,
  info,
  initialSetup,
}: {
  initial: SettingsValue;
  info: AppInfo;
  initialSetup: SetupState;
}) {
  const [settings, setSettings] = useState(initial);
  const [setup, setSetup] = useState(initialSetup);
  const [failed, setFailed] = useState<ReadonlyArray<ShortcutAction>>([]);
  const [recording, setRecording] = useState<ShortcutAction | null>(null);
  const [hint, setHint] = useState<string | null>(null);

  useEvent("settings:changed", setSettings);

  const refreshSetup = () =>
    run(invoke("setup:state").pipe(Effect.tap((value) => Effect.sync(() => setSetup(value)))));

  // Permission and macOS shortcut state can change outside ss; re-check when we regain focus.
  useEffect(() => {
    window.addEventListener("focus", refreshSetup);
    return () => window.removeEventListener("focus", refreshSetup);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const applyPreset = (preset: "system" | "alongside") =>
    run(
      invoke("setup:apply-shortcuts", { preset }).pipe(
        Effect.tap((result) =>
          Effect.sync(() => {
            setSettings(result.settings);
            setFailed(result.failedShortcuts);
          }),
        ),
        Effect.andThen(invoke("setup:state")),
        Effect.tap((value) => Effect.sync(() => setSetup(value))),
      ),
    );

  const setShortcut = (action: ShortcutAction, accelerator: string) =>
    run(
      invoke("settings:update", {
        shortcuts: { ...settings.shortcuts, [action]: accelerator },
      }).pipe(
        Effect.tap((result) =>
          Effect.sync(() => {
            setSettings(result.settings);
            setFailed(result.failedShortcuts);
          }),
        ),
      ),
    );

  // While recording a shortcut, release the global ones so pressing them doesn't fire a capture.
  useEffect(() => {
    if (recording === null) return;
    run(invoke("shortcuts:pause", { paused: true }));
    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      if (event.key === "Escape") return setRecording(null);
      if (event.key === "Backspace" && !event.metaKey && !event.altKey && !event.ctrlKey) {
        setShortcut(recording, "");
        return setRecording(null);
      }
      const accelerator = acceleratorFromEvent(event);
      if (accelerator === null) return;
      if (!isUsableAccelerator(accelerator)) {
        return setHint("Include ⌘ or ⌃ — macOS ignores shortcuts that only use ⌥ / ⇧.");
      }
      setHint(null);
      setShortcut(recording, accelerator);
      setRecording(null);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      setHint(null);
      window.removeEventListener("keydown", onKeyDown, true);
      run(invoke("shortcuts:pause", { paused: false }));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recording]);

  return (
    <div className="flex h-full flex-col bg-canvas">
      <header className="drag flex h-12 shrink-0 items-center justify-center">
        <span className="text-[13px] font-semibold">Settings</span>
      </header>

      <div className="flex-1 space-y-7 overflow-y-auto px-7 pt-3 pb-7 animate-rise-in">
        <Section
          title="System"
          aside={
            <button
              type="button"
              onClick={() => run(invoke("setup:open"))}
              className="text-[11.5px] font-medium text-accent hover:underline"
            >
              Open Setup Guide
            </button>
          }
        >
          <div className={cx(card, "divide-y divide-line")}>
            <div className="flex items-center justify-between gap-4 px-4 py-2.5">
              <div>
                <div className="text-[13px]">Screen Recording</div>
                <div className="text-[11.5px] text-muted">
                  {setup.permissionHost === null
                    ? "Required to capture your screen."
                    : `In development, granted to ${setup.permissionHost}.`}
                </div>
              </div>
              {setup.screenAccess === "granted" ? (
                <StatusPill ok>Granted</StatusPill>
              ) : (
                <Button
                  onClick={() =>
                    run(
                      invoke("setup:request-screen-access").pipe(
                        Effect.tap((value) => Effect.sync(() => setSetup(value))),
                      ),
                    )
                  }
                >
                  Grant Access…
                </Button>
              )}
            </div>
            <div className="flex items-center justify-between gap-4 px-4 py-2.5">
              <div>
                <div className="text-[13px]">macOS screenshot shortcuts</div>
                <div className="text-[11.5px] text-muted">
                  {setup.systemScreenshotShortcuts
                    ? "On — ⇧⌘3 / ⇧⌘4 / ⇧⌘5 open the built-in tool."
                    : "Off — ⇧⌘3 / ⇧⌘4 / ⇧⌘5 are free for ss."}
                </div>
              </div>
              {setup.systemScreenshotShortcuts ? (
                <Button onClick={() => applyPreset("system")}>Use for ss</Button>
              ) : (
                <Button onClick={() => applyPreset("alongside")}>Give back to macOS</Button>
              )}
            </div>
            <div className="flex items-center justify-between gap-4 px-4 py-2.5">
              <div>
                <div className="text-[13px]">Launch at login</div>
                <div className="text-[11.5px] text-muted">
                  {setup.launchAtLogin.available
                    ? "Keeps your shortcuts working after a restart."
                    : "Available in the installed app."}
                </div>
              </div>
              <Switch
                checked={setup.launchAtLogin.enabled}
                disabled={!setup.launchAtLogin.available}
                onChange={(enabled) =>
                  run(
                    invoke("setup:launch-at-login", { enabled }).pipe(
                      Effect.tap((value) => Effect.sync(() => setSetup(value))),
                    ),
                  )
                }
              />
            </div>
          </div>
        </Section>

        <Section title="Shortcuts">
          <div className="divide-y divide-line overflow-hidden rounded-xl bg-white shadow-[0_0_0_0.5px_rgb(0_0_0/0.08)]">
            {SHORTCUTS.map(({ action, label }) => {
              const isRecording = recording === action;
              const isFailed = failed.includes(action);
              return (
                <div key={action} className="flex h-11 items-center justify-between px-4">
                  <span className="text-[13px]">{label}</span>
                  <div className="flex items-center gap-2">
                    {isFailed && <span className="text-[11px] text-red-500">in use elsewhere</span>}
                    <button
                      type="button"
                      onClick={() => setRecording(isRecording ? null : action)}
                      className={cx(
                        "h-7 min-w-[92px] rounded-lg px-2.5 text-[12.5px] font-medium tabular-nums transition-all duration-200 ease-out-soft",
                        isRecording
                          ? "bg-accent-soft text-accent ring-1 ring-accent/40"
                          : "bg-black/[0.045] text-ink hover:bg-black/[0.07]",
                        isFailed && !isRecording && "text-red-500",
                      )}
                    >
                      {isRecording ? "Press keys…" : formatAccelerator(settings.shortcuts[action])}
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
          <p className={cx("text-[11.5px] leading-relaxed", hint ? "text-red-500" : "text-muted")}>
            {hint ?? "Click a shortcut and press a new combination. ⌫ clears it, Esc cancels."}
          </p>
        </Section>

        <Section title="After capture">
          <Segmented
            options={AFTER_CAPTURE}
            value={settings.afterCapture}
            onChange={(afterCapture) =>
              run(
                invoke("settings:update", { afterCapture }).pipe(
                  Effect.tap((result) => Effect.sync(() => setSettings(result.settings))),
                ),
              )
            }
          />
          <p className="text-[11.5px] text-muted">
            Every capture goes straight to the clipboard. Nothing is ever saved to the Desktop.
          </p>
        </Section>

        <Section title="Folders">
          <div className="divide-y divide-line overflow-hidden rounded-xl bg-white shadow-[0_0_0_0.5px_rgb(0_0_0/0.08)]">
            <FolderRow
              label="Saved screenshots (⌘S)"
              path={settings.imagesDirectory ?? info.defaultImagesDirectory}
              onChange={() => run(invoke("settings:pick-directory", { kind: "images" }))}
            />
            <FolderRow
              label="Recordings"
              path={settings.recordingsDirectory ?? info.defaultRecordingsDirectory}
              onChange={() => run(invoke("settings:pick-directory", { kind: "recordings" }))}
            />
          </div>
        </Section>

        <p className="text-center text-[11px] text-muted">ss {info.version}</p>
      </div>
    </div>
  );
}

function FolderRow({
  label,
  path,
  onChange,
}: {
  label: string;
  path: string;
  onChange: () => void;
}) {
  return (
    <div className="flex items-center justify-between gap-4 px-4 py-2.5">
      <div className="min-w-0">
        <div className="text-[13px]">{label}</div>
        <div className="truncate font-mono text-[11px] text-muted" title={path}>
          {tildify(path)}
        </div>
      </div>
      <Button onClick={onChange}>Change…</Button>
    </div>
  );
}
