import { Context, Effect, FileSystem, Layer, Option, Scope } from "effect";
import { ChildProcess, ChildProcessSpawner } from "effect/process";

import type { Rect } from "../shared/ipc";
import { CaptureError, RecorderError, ScreenPermissionError } from "./errors";
import { runCollect } from "./Process";

/**
 * Thin wrapper around macOS' own `screencapture` binary. It gives us the native selection UI
 * (crosshair, magnifier, Space for window mode) and hardware-encoded recordings, while we keep
 * full control over where files go — never the Desktop.
 */
export interface RecordingProcess {
  /** Ask screencapture to finish and finalize the movie file. */
  readonly stop: Effect.Effect<void>;
  /** Resolves when the process exits on its own (e.g. user stopped from the menu bar). */
  readonly exited: Effect.Effect<number>;
}

const rectArg = (rect: Rect) =>
  `-R${[rect.x, rect.y, rect.width, rect.height].map((value) => Math.round(value)).join(",")}`;

const looksLikePermissionError = (stderr: string) =>
  /could not create image|not authorized|permission|declined/i.test(stderr);

export class Screencapture extends Context.Service<
  Screencapture,
  {
    /**
     * Native interactive selection (drag for area, Space for window, Esc to cancel).
     * Returns `None` when the user cancels.
     */
    interactive(
      file: string,
    ): Effect.Effect<Option.Option<string>, CaptureError | ScreenPermissionError>;
    /** Capture a rectangle in global screen points, e.g. a whole display. */
    rect(file: string, rect: Rect): Effect.Effect<string, CaptureError | ScreenPermissionError>;
    /** Start recording a rectangle in global screen points. The process lives in the given scope. */
    record(file: string, rect: Rect): Effect.Effect<RecordingProcess, RecorderError, Scope.Scope>;
  }
>()("ss/main/Screencapture") {
  static readonly layer = Layer.effect(
    Screencapture,
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const spawner = yield* ChildProcessSpawner.ChildProcessSpawner;
      const provide = Effect.provideService(ChildProcessSpawner.ChildProcessSpawner, spawner);

      const run = Effect.fnUntraced(function* (args: ReadonlyArray<string>, file: string) {
        const result = yield* runCollect("screencapture", [...args, file]).pipe(
          provide,
          Effect.mapError(
            (cause) => new CaptureError({ message: "Cannot run screencapture", cause }),
          ),
        );
        const exists = yield* fs.exists(file).pipe(Effect.orElseSucceed(() => false));
        if (exists) return Option.some(file);
        if (looksLikePermissionError(result.stderr)) {
          return yield* new ScreenPermissionError({ detail: result.stderr });
        }
        if (result.stderr !== "") {
          return yield* new CaptureError({ message: result.stderr });
        }
        return Option.none<string>();
      });

      const interactive = Effect.fn("Screencapture.interactive")(function* (file: string) {
        // -i interactive, -x silent, -o no window shadow (we add our own in the editor)
        return yield* run(["-i", "-x", "-o"], file);
      });

      const rect = Effect.fn("Screencapture.rect")(function* (file: string, area: Rect) {
        const result = yield* run(["-x", rectArg(area)], file);
        if (Option.isNone(result))
          return yield* new CaptureError({ message: "Nothing was captured" });
        return result.value;
      });

      const record = Effect.fn("Screencapture.record")(function* (file: string, area: Rect) {
        const handle = yield* spawner
          .spawn(
            ChildProcess.make("screencapture", ["-v", "-x", rectArg(area), file], {
              // Keep stdin open: screencapture treats input/EOF on stdin as "stop recording".
              stdin: "pipe",
              // Interrupt (not terminate) on scope close so the movie gets finalized.
              killSignal: "SIGINT",
            }),
          )
          .pipe(
            Effect.mapError(
              (cause) => new RecorderError({ message: "Cannot start recording", cause }),
            ),
          );

        const exited = handle.exitCode.pipe(
          Effect.map(Number),
          Effect.orElseSucceed(() => -1),
        );

        // screencapture stops on "any character" or Ctrl-C; try the polite way first.
        const stop = Effect.gen(function* () {
          yield* Effect.logInfo("Stopping recording");
          yield* handle.kill({ killSignal: "SIGINT" }).pipe(Effect.ignore);
          const finished = yield* exited.pipe(Effect.timeoutOption("5 seconds"));
          if (Option.isNone(finished)) {
            yield* Effect.logWarning("screencapture ignored SIGINT, terminating");
            yield* handle.kill({ killSignal: "SIGTERM" }).pipe(Effect.ignore);
            yield* exited.pipe(Effect.timeoutOption("2 seconds"));
          }
        });

        return { stop, exited } satisfies RecordingProcess;
      });

      return Screencapture.of({ interactive, rect, record });
    }),
  );
}
