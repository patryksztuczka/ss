import { Context, Effect, Layer } from "effect";
import { ChildProcessSpawner } from "effect/process";

import { HelperError } from "./errors";
import { Paths } from "./Paths";
import { runCollect } from "./Process";

/** Runs the bundled `ss-helper` binary (native/helper.swift) and returns its stdout. */
export class NativeHelper extends Context.Service<
  NativeHelper,
  {
    run(args: ReadonlyArray<string>): Effect.Effect<string, HelperError>;
  }
>()("ss/main/NativeHelper") {
  static readonly layer = Layer.effect(
    NativeHelper,
    Effect.gen(function* () {
      const paths = yield* Paths;
      const spawner = yield* ChildProcessSpawner.ChildProcessSpawner;

      const run = Effect.fn("NativeHelper.run")(function* (args: ReadonlyArray<string>) {
        const result = yield* runCollect(paths.helperBinary, args).pipe(
          Effect.provideService(ChildProcessSpawner.ChildProcessSpawner, spawner),
          Effect.mapError((cause) => new HelperError({ message: "Cannot run ss-helper", cause })),
        );
        if (result.exitCode !== 0) {
          return yield* new HelperError({
            message: result.stderr || `ss-helper ${args[0]} failed`,
          });
        }
        return result.stdout.trim();
      });

      return NativeHelper.of({ run });
    }),
  );
}
