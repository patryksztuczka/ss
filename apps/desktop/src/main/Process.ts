import { Effect, Stream } from "effect";
import { ChildProcess, ChildProcessSpawner } from "effect/process";

export interface ProcessOutput {
  readonly exitCode: number;
  readonly stdout: string;
  readonly stderr: string;
}

/** Runs a command to completion and collects its exit code and both output streams. */
export const runCollect = Effect.fnUntraced(function* (
  command: string,
  args: ReadonlyArray<string>,
) {
  const spawner = yield* ChildProcessSpawner.ChildProcessSpawner;
  const handle = yield* spawner.spawn(ChildProcess.make(command, args));
  const [stdout, stderr, exitCode] = yield* Effect.all(
    [
      Stream.mkString(Stream.decodeText(handle.stdout)),
      Stream.mkString(Stream.decodeText(handle.stderr)),
      handle.exitCode,
    ],
    { concurrency: "unbounded" },
  );
  return { exitCode: Number(exitCode), stdout, stderr: stderr.trim() } satisfies ProcessOutput;
}, Effect.scoped);
