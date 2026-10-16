import { dirname } from "node:path";
import { Context, Effect, FileSystem, Layer, Schema, Stream, SubscriptionRef } from "effect";

import { Settings, defaultSettings, repairShortcuts, type SettingsPatch } from "../shared/settings";
import { StorageError } from "./errors";
import { Paths } from "./Paths";

const SettingsJson = Schema.fromJsonString(Settings, { space: 2 });
const decodeSettingsJson = Schema.decodeUnknownEffect(SettingsJson);
const encodeSettingsJson = Schema.encodeEffect(SettingsJson);

export class SettingsStore extends Context.Service<
  SettingsStore,
  {
    readonly get: Effect.Effect<Settings>;
    readonly changes: Stream.Stream<Settings>;
    update(patch: SettingsPatch): Effect.Effect<Settings, StorageError>;
  }
>()("ss/main/SettingsStore") {
  static readonly layer = Layer.effect(
    SettingsStore,
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const paths = yield* Paths;

      const initial = yield* fs.readFileString(paths.settingsFile).pipe(
        Effect.flatMap(decodeSettingsJson),
        Effect.catch((error) =>
          Effect.logDebug("Using default settings", error).pipe(Effect.as(defaultSettings)),
        ),
      );
      const repaired = { ...initial, shortcuts: repairShortcuts(initial.shortcuts) };
      const ref = yield* SubscriptionRef.make(repaired);

      const persist = Effect.fn("SettingsStore.persist")(
        function* (settings: Settings) {
          const json = yield* encodeSettingsJson(settings);
          yield* fs.makeDirectory(dirname(paths.settingsFile), { recursive: true });
          yield* fs.writeFileString(paths.settingsFile, json);
        },
        Effect.mapError((cause) => new StorageError({ message: "Could not save settings", cause })),
      );

      const update = Effect.fn("SettingsStore.update")(function* (patch: SettingsPatch) {
        const next = yield* SubscriptionRef.updateAndGet(ref, (current) => ({
          ...current,
          ...patch,
        }));
        yield* persist(next);
        return next;
      });

      if (JSON.stringify(repaired.shortcuts) !== JSON.stringify(initial.shortcuts)) {
        yield* Effect.logInfo("Replaced shortcuts macOS ignores (⌥/⌥⇧ only)");
        yield* persist(repaired).pipe(Effect.ignore);
      }

      return SettingsStore.of({
        get: SubscriptionRef.get(ref),
        changes: SubscriptionRef.changes(ref),
        update,
      });
    }),
  );
}
