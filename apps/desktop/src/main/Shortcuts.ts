import { globalShortcut } from "electron";
import { Context, Effect, Layer } from "effect";

import {
  isUsableAccelerator,
  type ShortcutAction,
  type Shortcuts as ShortcutMap,
} from "../shared/settings";
import { Actions } from "./Actions";
import { SettingsStore } from "./SettingsStore";

export class Shortcuts extends Context.Service<
  Shortcuts,
  {
    /** Registers the map and returns the actions whose accelerator could not be registered. */
    apply(map: ShortcutMap): Effect.Effect<ReadonlyArray<ShortcutAction>>;
    /** Temporarily releases all global shortcuts (while recording a new one in Settings). */
    setPaused(paused: boolean): Effect.Effect<void>;
  }
>()("ss/main/Shortcuts") {
  static readonly layer = Layer.effect(
    Shortcuts,
    Effect.gen(function* () {
      const actions = yield* Actions;
      const settings = yield* SettingsStore;
      const context = yield* Effect.context<never>();
      const run = Effect.runForkWith(context);

      const handlers: Record<ShortcutAction, Effect.Effect<void>> = {
        captureArea: actions.captureArea,
        captureScreen: actions.captureScreen,
        recordArea: actions.recordArea,
        recordScreen: actions.recordScreen,
        openLast: actions.openLast,
      };

      const register = (map: ShortcutMap) =>
        Effect.sync(() => {
          globalShortcut.unregisterAll();
          const failed: Array<ShortcutAction> = [];
          for (const [action, accelerator] of Object.entries(map) as Array<
            [ShortcutAction, string]
          >) {
            if (accelerator === "") continue;
            if (!isUsableAccelerator(accelerator)) {
              failed.push(action);
              continue;
            }
            try {
              if (!globalShortcut.register(accelerator, () => void run(handlers[action])))
                failed.push(action);
            } catch {
              // Electron throws on syntactically invalid accelerators.
              failed.push(action);
            }
          }
          return failed;
        });

      const apply = Effect.fn("Shortcuts.apply")(function* (map: ShortcutMap) {
        const failed = yield* register(map);
        if (failed.length > 0) yield* Effect.logWarning("Shortcuts not registered", failed);
        return failed;
      });

      const setPaused = Effect.fn("Shortcuts.setPaused")(function* (value: boolean) {
        if (value) return yield* Effect.sync(() => globalShortcut.unregisterAll());
        yield* apply((yield* settings.get).shortcuts);
      });

      yield* Effect.addFinalizer(() => Effect.sync(() => globalShortcut.unregisterAll()));

      yield* apply((yield* settings.get).shortcuts);

      return Shortcuts.of({ apply, setPaused });
    }),
  );
}
