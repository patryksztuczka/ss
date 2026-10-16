import {
  Clock,
  Context,
  Effect,
  Exit,
  FileSystem,
  Layer,
  Option,
  Ref,
  Scope,
  Stream,
  SubscriptionRef,
} from "effect";

import type { Rect } from "../shared/ipc";
import { CaptureStore, type Capture } from "./CaptureStore";
import { RecorderError, StorageError } from "./errors";
import { Paths } from "./Paths";
import { Screencapture, type RecordingProcess } from "./Screencapture";
import { SettingsStore } from "./SettingsStore";

export type RecorderState =
  | { readonly _tag: "Idle" }
  | { readonly _tag: "Recording"; readonly startedAt: number };

interface Active {
  readonly capture: Capture;
  readonly process: RecordingProcess;
}

export class Recorder extends Context.Service<
  Recorder,
  {
    readonly state: Effect.Effect<RecorderState>;
    readonly changes: Stream.Stream<RecorderState>;
    /**
     * Starts recording `rect` (global points). `onFinish` runs once the movie file is finalized,
     * whether the recording was stopped by us or ended on its own.
     */
    start(
      rect: Rect,
      onFinish: (capture: Capture) => Effect.Effect<void>,
    ): Effect.Effect<void, RecorderError | StorageError>;
    /** Stops the current recording (no-op when idle). */
    readonly stop: Effect.Effect<void>;
  }
>()("ss/main/Recorder") {
  static readonly layer = Layer.effect(
    Recorder,
    Effect.gen(function* () {
      const screencapture = yield* Screencapture;
      const store = yield* CaptureStore;
      const settings = yield* SettingsStore;
      const paths = yield* Paths;
      const fs = yield* FileSystem.FileSystem;

      const state = yield* SubscriptionRef.make<RecorderState>({ _tag: "Idle" });
      const active = yield* Ref.make(Option.none<Active>());

      const start = Effect.fn("Recorder.start")(function* (
        rect: Rect,
        onFinish: (capture: Capture) => Effect.Effect<void>,
      ) {
        if (Option.isSome(yield* Ref.get(active))) {
          return yield* new RecorderError({ message: "Already recording" });
        }
        const directory = (yield* settings.get).recordingsDirectory ?? paths.defaultRecordings;
        const capture = yield* store.newRecording(directory);
        const scope = yield* Scope.make();
        const process = yield* screencapture.record(capture.path, rect).pipe(
          Scope.provide(scope),
          Effect.onError(() => Scope.close(scope, Exit.void)),
        );

        const recording = { ...capture, size: { width: rect.width, height: rect.height } };
        yield* Ref.set(active, Option.some({ capture: recording, process }));
        yield* SubscriptionRef.set(state, {
          _tag: "Recording",
          startedAt: yield* Clock.currentTimeMillis,
        });
        yield* Effect.logInfo("Recording started", capture.path);

        // Single place that finalizes a recording, however it ended.
        yield* process.exited.pipe(
          Effect.andThen(Scope.close(scope, Exit.void)),
          Effect.andThen(Ref.set(active, Option.none())),
          Effect.andThen(SubscriptionRef.set(state, { _tag: "Idle" })),
          Effect.andThen(fs.exists(capture.path).pipe(Effect.orElseSucceed(() => false))),
          Effect.flatMap((exists) =>
            exists
              ? store.register(recording).pipe(Effect.andThen(onFinish(recording)))
              : Effect.logWarning("Recording produced no file"),
          ),
          Effect.forkDetach,
        );
      });

      const stop = Ref.get(active).pipe(
        Effect.flatMap(
          Option.match({
            onNone: () => Effect.void,
            onSome: ({ process }) => process.stop,
          }),
        ),
      );

      // Never leave screencapture running behind our back when ss quits.
      yield* Effect.addFinalizer(() => stop);

      return Recorder.of({
        state: SubscriptionRef.get(state),
        changes: SubscriptionRef.changes(state),
        start,
        stop,
      });
    }),
  );
}
