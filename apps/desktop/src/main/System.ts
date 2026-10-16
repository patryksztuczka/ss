import { app, shell, systemPreferences } from "electron";
import { Context, Effect, Layer } from "effect";

import type { ScreenAccess, SetupState } from "../shared/ipc";
import type { HelperError } from "./errors";
import { NativeHelper } from "./NativeHelper";

const SCREEN_RECORDING_SETTINGS =
  "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";

const TERMINALS: Record<string, string> = {
  "com.apple.Terminal": "Terminal",
  "com.googlecode.iterm2": "iTerm",
  "com.mitchellh.ghostty": "Ghostty",
  "com.microsoft.VSCode": "Visual Studio Code",
  "com.todesktop.230313mzl4w4u92": "Cursor",
  "dev.warp.Warp-Stable": "Warp",
  "dev.zed.Zed": "Zed",
  "com.t3tools.t3code": "T3 Code",
};

/** The app macOS attributes our permission requests to while running unpackaged. */
const permissionHost = (): string | null => {
  if (app.isPackaged) return null;
  const bundleId = process.env.__CFBundleIdentifier;
  if (bundleId !== undefined) return TERMINALS[bundleId] ?? bundleId;
  return process.env.TERM_PROGRAM ?? "your terminal app";
};

/** Permissions and OS integration: Screen Recording, macOS screenshot keys, launch at login. */
export class System extends Context.Service<
  System,
  {
    readonly screenAccess: Effect.Effect<ScreenAccess>;
    /** Shows the macOS prompt (first time only) and opens System Settings if still denied. */
    readonly requestScreenAccess: Effect.Effect<ScreenAccess>;
    readonly openScreenRecordingSettings: Effect.Effect<void>;
    readonly systemScreenshotShortcuts: Effect.Effect<boolean, HelperError>;
    setSystemScreenshotShortcuts(enabled: boolean): Effect.Effect<boolean, HelperError>;
    setLaunchAtLogin(enabled: boolean): Effect.Effect<void>;
    readonly state: Effect.Effect<SetupState>;
  }
>()("ss/main/System") {
  static readonly layer = Layer.effect(
    System,
    Effect.gen(function* () {
      const helper = yield* NativeHelper;

      const asAccess = (value: string): ScreenAccess =>
        value === "granted" ? "granted" : "denied";

      // A fresh helper process sees permission changes immediately; Electron's own check is
      // cached for the lifetime of this process, so it's only the fallback.
      const screenAccess = helper.run(["screen-access"]).pipe(
        Effect.map(asAccess),
        Effect.orElseSucceed(() => asAccess(systemPreferences.getMediaAccessStatus("screen"))),
      );

      const openScreenRecordingSettings = Effect.promise(() =>
        shell.openExternal(SCREEN_RECORDING_SETTINGS),
      );

      const requestScreenAccess = Effect.gen(function* () {
        const result = yield* helper.run(["request-screen-access"]).pipe(
          Effect.map(asAccess),
          Effect.timeoutOption("15 seconds"),
          Effect.map((value) => (value._tag === "Some" ? value.value : "denied")),
          Effect.orElseSucceed((): ScreenAccess => "denied"),
        );
        if (result === "denied") yield* openScreenRecordingSettings;
        return result;
      }).pipe(Effect.withSpan("System.requestScreenAccess"));

      const systemScreenshotShortcuts = helper
        .run(["screenshot-shortcuts"])
        .pipe(Effect.map((value) => value === "enabled"));

      const setSystemScreenshotShortcuts = (enabled: boolean) =>
        helper.run(["screenshot-shortcuts", enabled ? "on" : "off"]).pipe(
          Effect.map((value) => value === "enabled"),
          Effect.tap((value) => Effect.logInfo("macOS screenshot shortcuts", value ? "on" : "off")),
        );

      const setLaunchAtLogin = (enabled: boolean) =>
        Effect.sync(() => {
          if (app.isPackaged) app.setLoginItemSettings({ openAtLogin: enabled });
        });

      const state = Effect.gen(function* () {
        const [access, shortcuts] = yield* Effect.all(
          [screenAccess, systemScreenshotShortcuts.pipe(Effect.orElseSucceed(() => true))],
          { concurrency: "unbounded" },
        );
        return {
          screenAccess: access,
          systemScreenshotShortcuts: shortcuts,
          launchAtLogin: {
            available: app.isPackaged,
            enabled: app.isPackaged && app.getLoginItemSettings().openAtLogin,
          },
          permissionHost: permissionHost(),
        } satisfies SetupState;
      });

      return System.of({
        screenAccess,
        requestScreenAccess,
        openScreenRecordingSettings,
        systemScreenshotShortcuts,
        setSystemScreenshotShortcuts,
        setLaunchAtLogin,
        state,
      });
    }),
  );
}
