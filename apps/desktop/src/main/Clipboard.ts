import { clipboard, ClipboardItem } from "electron";
import { Context, Effect, FileSystem, Layer } from "effect";
import { ChildProcessSpawner } from "effect/process";

import { StorageError } from "./errors";
import { runCollect } from "./Process";

const COPY_FILE_SCRIPT = [
  "on run argv",
  "set the clipboard to (POSIX file (item 1 of argv))",
  "end run",
].flatMap((line) => ["-e", line]);

export class Clipboard extends Context.Service<
  Clipboard,
  {
    imageFromFile(path: string): Effect.Effect<void, StorageError>;
    imageFromPng(png: Uint8Array): Effect.Effect<void, StorageError>;
    text(text: string): Effect.Effect<void>;
    /** Puts a file reference on the pasteboard (paste into Finder, Slack, Mail…). */
    file(path: string): Effect.Effect<void, StorageError>;
  }
>()("ss/main/Clipboard") {
  static readonly layer = Layer.effect(
    Clipboard,
    Effect.gen(function* () {
      const spawner = yield* ChildProcessSpawner.ChildProcessSpawner;

      const fs = yield* FileSystem.FileSystem;

      const imageFromPng = (png: Uint8Array) =>
        Effect.tryPromise({
          try: () =>
            clipboard.write([
              new ClipboardItem({
                "image/png": new Blob([new Uint8Array(png)], { type: "image/png" }),
              }),
            ]),
          catch: (cause) => new StorageError({ message: "Cannot copy image to clipboard", cause }),
        });

      return Clipboard.of({
        imageFromPng,
        imageFromFile: (path) =>
          fs.readFile(path).pipe(
            Effect.mapError((cause) => new StorageError({ message: "Cannot read capture", cause })),
            Effect.flatMap(imageFromPng),
          ),
        text: (text) => Effect.promise(() => clipboard.writeText(text)),
        file: Effect.fn("Clipboard.file")(function* (path: string) {
          const result = yield* runCollect("osascript", [...COPY_FILE_SCRIPT, path]).pipe(
            Effect.provideService(ChildProcessSpawner.ChildProcessSpawner, spawner),
            Effect.mapError(
              (cause) => new StorageError({ message: "Cannot copy file to clipboard", cause }),
            ),
          );
          if (result.exitCode !== 0) return yield* new StorageError({ message: result.stderr });
        }),
      });
    }),
  );
}
