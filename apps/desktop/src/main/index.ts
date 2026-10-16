import { join } from "node:path";
import { NodeServices } from "@effect/platform-node";
import { app } from "electron";
import { Effect, Fiber, Layer } from "effect";

import { Actions } from "./Actions";
import { CaptureStore } from "./CaptureStore";
import { Clipboard } from "./Clipboard";
import { IpcLive } from "./Ipc";
import { NativeHelper } from "./NativeHelper";
import { Ocr } from "./Ocr";
import { Paths } from "./Paths";
import { Recorder } from "./Recorder";
import { Screencapture } from "./Screencapture";
import { SettingsStore } from "./SettingsStore";
import { Shortcuts } from "./Shortcuts";
import { System } from "./System";
import { TrayLive } from "./Tray";
import { Windows } from "./Windows";

const CoreLive = Layer.mergeAll(
  SettingsStore.layer,
  CaptureStore.layer,
  Screencapture.layer,
  Clipboard.layer,
  NativeHelper.layer,
).pipe(Layer.provideMerge(Paths.layer), Layer.provideMerge(NodeServices.layer));

const ServicesLive = Layer.mergeAll(Ocr.layer, Recorder.layer, Windows.layer, System.layer).pipe(
  Layer.provideMerge(CoreLive),
);

const ActionsLive = Actions.layer.pipe(Layer.provideMerge(ServicesLive));

/** First launch: walk through permissions and shortcuts. */
const OnboardingLive = Layer.effectDiscard(
  Effect.gen(function* () {
    const settings = yield* SettingsStore;
    const windows = yield* Windows;
    if (!(yield* settings.get).onboarded) yield* windows.openOnboarding;
  }),
);

const AppLive = Layer.mergeAll(TrayLive, IpcLive, OnboardingLive).pipe(
  Layer.provide(Shortcuts.layer.pipe(Layer.provideMerge(ActionsLive))),
);

const main = Effect.gen(function* () {
  yield* Effect.promise(() => app.whenReady());
  app.dock?.hide();
  yield* Effect.logInfo("ss is ready");
  return yield* Layer.launch(AppLive);
});

// Same identity (and data folder) in development and in the packaged app.
app.setName("ss");
app.setPath("userData", join(app.getPath("appData"), "ss"));

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  // A menu bar app keeps running with no windows open.
  app.on("window-all-closed", () => {});

  const fiber = Effect.runFork(main);
  let shuttingDown = false;

  // Run layer finalizers (stop recording, release shortcuts, remove tray) before exiting.
  app.on("before-quit", (event) => {
    if (shuttingDown) return;
    shuttingDown = true;
    event.preventDefault();
    Effect.runFork(
      Fiber.interrupt(fiber).pipe(
        Effect.timeout("5 seconds"),
        Effect.ignore,
        // Finalizers can complete synchronously inside this very handler, where a nested
        // app.quit() is ignored — so quit again on the next tick.
        Effect.ensuring(Effect.sync(() => setImmediate(() => app.quit()))),
      ),
    );
  });
}
