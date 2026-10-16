import { join } from "node:path";
import { Cache, Context, Duration, Effect, Exit, FileSystem, Layer, Schema } from "effect";

import type { CaptureId } from "../shared/ipc";
import { TextLayout } from "../shared/ocr";
import { CaptureStore } from "./CaptureStore";
import { CaptureNotFound, OcrError } from "./errors";
import { Paths } from "./Paths";
import { NativeHelper } from "./NativeHelper";

const TextLayoutJson = Schema.fromJsonString(TextLayout);
const decodeLayout = Schema.decodeUnknownEffect(TextLayoutJson);
const encodeLayout = Schema.encodeEffect(TextLayoutJson);

/** Reading order: top-to-bottom rows, then left-to-right within a row. */
const sortLines = (layout: TextLayout): TextLayout => ({
  ...layout,
  lines: [...layout.lines].sort((a, b) => {
    const sameRow = Math.abs(a.y + a.h / 2 - (b.y + b.h / 2)) < Math.min(a.h, b.h) / 2;
    return sameRow ? a.x - b.x : a.y - b.y;
  }),
});

/** On-device text recognition (Apple Vision) via the bundled `ss-helper` binary. */
export class Ocr extends Context.Service<
  Ocr,
  {
    recognize(id: CaptureId): Effect.Effect<TextLayout, OcrError | CaptureNotFound>;
  }
>()("ss/main/Ocr") {
  static readonly layer = Layer.effect(
    Ocr,
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const helper = yield* NativeHelper;
      const paths = yield* Paths;
      const store = yield* CaptureStore;

      const recognizeFile = Effect.fn("Ocr.recognizeFile")(function* (image: string) {
        const stdout = yield* helper
          .run(["ocr", image])
          .pipe(Effect.mapError((cause) => new OcrError({ message: cause.message, cause })));
        return yield* decodeLayout(stdout).pipe(
          Effect.mapError((cause) => new OcrError({ message: "Malformed OCR output", cause })),
        );
      });

      const lookup = Effect.fn("Ocr.lookup")(function* (id: CaptureId) {
        const capture = yield* store.get(id);
        const cacheFile = join(paths.captures, `${id}.ocr.json`);
        const cached = yield* fs
          .readFileString(cacheFile)
          .pipe(Effect.flatMap(decodeLayout), Effect.option);
        if (cached._tag === "Some") return cached.value;

        const layout = sortLines(yield* recognizeFile(capture.path));
        yield* encodeLayout(layout).pipe(
          Effect.flatMap((json) => fs.writeFileString(cacheFile, json)),
          Effect.ignore,
        );
        return layout;
      });

      // Dedupes the background prefetch and the editor's request for the same capture.
      const cache = yield* Cache.makeWith(lookup, {
        capacity: 32,
        timeToLive: (exit) => (Exit.isSuccess(exit) ? Duration.hours(1) : Duration.zero),
      });

      return Ocr.of({ recognize: (id) => Cache.get(cache, id) });
    }),
  );
}
