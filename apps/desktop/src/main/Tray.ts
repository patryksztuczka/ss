import { join } from "node:path";
import { app, Menu, nativeImage, Tray as ElectronTray } from "electron";
import { Clock, Effect, FiberHandle, Layer, Schedule, Stream } from "effect";

import type { Settings } from "../shared/settings";
import { Actions } from "./Actions";
import { Paths } from "./Paths";
import { Recorder } from "./Recorder";
import { SettingsStore } from "./SettingsStore";

const formatElapsed = (ms: number) => {
  const total = Math.floor(ms / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
};

/** Menu bar presence: the only always-visible UI of ss. */
export const TrayLive = Layer.effectDiscard(
  Effect.gen(function* () {
    const paths = yield* Paths;
    const actions = yield* Actions;
    const recorder = yield* Recorder;
    const settings = yield* SettingsStore;
    const context = yield* Effect.context<never>();
    const run = Effect.runForkWith(context);

    const icon = nativeImage.createFromPath(join(paths.resources, "trayTemplate.png"));
    icon.setTemplateImage(true);
    const tray = yield* Effect.acquireRelease(
      Effect.sync(() => new ElectronTray(icon)),
      (instance) => Effect.sync(() => instance.destroy()),
    );
    tray.setToolTip("ss");

    let menu = Menu.buildFromTemplate([]);
    const buildMenu = ({ shortcuts }: Settings) =>
      Menu.buildFromTemplate([
        {
          label: "Capture Area",
          accelerator: shortcuts.captureArea,
          registerAccelerator: false,
          click: () => void run(actions.captureArea),
        },
        {
          label: "Capture Screen",
          accelerator: shortcuts.captureScreen,
          registerAccelerator: false,
          click: () => void run(actions.captureScreen),
        },
        { type: "separator" },
        {
          label: "Record Area",
          accelerator: shortcuts.recordArea,
          registerAccelerator: false,
          click: () => void run(actions.recordArea),
        },
        {
          label: "Record Screen",
          accelerator: shortcuts.recordScreen,
          registerAccelerator: false,
          click: () => void run(actions.recordScreen),
        },
        { type: "separator" },
        {
          label: "Edit Last Capture",
          accelerator: shortcuts.openLast,
          registerAccelerator: false,
          click: () => void run(actions.openLast),
        },
        { label: "Open Saved Screenshots", click: () => void run(actions.openFolder("images")) },
        { label: "Open Recordings", click: () => void run(actions.openFolder("recordings")) },
        { type: "separator" },
        { label: "Setup Guide…", click: () => void run(actions.openOnboarding) },
        {
          label: "Settings…",
          accelerator: "Command+,",
          click: () => void run(actions.openSettings),
        },
        { label: "Quit ss", accelerator: "Command+Q", click: () => app.quit() },
      ]);

    yield* settings.changes.pipe(
      Stream.runForEach((value) => Effect.sync(() => (menu = buildMenu(value)))),
      Effect.forkScoped,
    );

    tray.on(
      "click",
      () =>
        void run(
          recorder.state.pipe(
            Effect.flatMap((state) =>
              state._tag === "Recording"
                ? actions.stopRecording
                : Effect.sync(() => tray.popUpContextMenu(menu)),
            ),
          ),
        ),
    );
    tray.on("right-click", () => tray.popUpContextMenu(menu));

    // While recording: "● 0:42" next to the icon; clicking the icon stops.
    const ticker = yield* FiberHandle.make();
    yield* recorder.changes.pipe(
      Stream.runForEach((state) => {
        if (state._tag === "Idle") {
          return FiberHandle.clear(ticker).pipe(
            Effect.andThen(
              Effect.sync(() => {
                tray.setTitle("");
                tray.setToolTip("ss");
              }),
            ),
          );
        }
        tray.setToolTip("Click to stop recording");
        return FiberHandle.run(
          ticker,
          Clock.currentTimeMillis.pipe(
            Effect.flatMap((now) =>
              Effect.sync(() => tray.setTitle(`● ${formatElapsed(now - state.startedAt)}`)),
            ),
            Effect.repeat(Schedule.spaced("1 second")),
          ),
        ).pipe(Effect.asVoid);
      }),
      Effect.forkScoped,
    );
  }),
);
