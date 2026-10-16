import { join } from "node:path";
import { Context, DateTime, Effect, FileSystem, Layer, Option, Random, Ref } from "effect";

import type { CaptureId, CaptureKind } from "../shared/ipc";
import { CaptureNotFound, StorageError } from "./errors";
import { Paths } from "./Paths";

export interface Capture {
  readonly id: CaptureId;
  readonly kind: CaptureKind;
  /** Absolute path of the image (in the history folder) or the recording. */
  readonly path: string;
  /** Pixel size for images, point size for recordings; unknown until captured. */
  readonly size?: { readonly width: number; readonly height: number } | undefined;
}

const KEEP_IMAGES = 60;

const pad = (value: number) => String(value).padStart(2, "0");

export class CaptureStore extends Context.Service<
  CaptureStore,
  {
    /** Reserve a path for a new screenshot. The file is only registered once `register` is called. */
    readonly newImage: Effect.Effect<Capture, StorageError>;
    /** Reserve a path for a new recording inside the recordings directory. */
    newRecording(directory: string): Effect.Effect<Capture, StorageError>;
    register(capture: Capture): Effect.Effect<void>;
    get(id: CaptureId): Effect.Effect<Capture, CaptureNotFound>;
    readonly latest: Effect.Effect<Option.Option<Capture>>;
    /** Human friendly name, e.g. "ss 2026-10-12 at 20.45.03". */
    readonly timestampName: (prefix: string) => Effect.Effect<string>;
  }
>()("ss/main/CaptureStore") {
  static readonly layer = Layer.effect(
    CaptureStore,
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      const paths = yield* Paths;
      const byId = yield* Ref.make(new Map<CaptureId, Capture>());
      const latestId = yield* Ref.make(Option.none<CaptureId>());

      const ensureDir = (dir: string) =>
        fs
          .makeDirectory(dir, { recursive: true })
          .pipe(
            Effect.mapError(
              (cause) => new StorageError({ message: `Cannot create ${dir}`, cause }),
            ),
          );

      // Rebuild the image history from disk so "open last" survives restarts.
      yield* ensureDir(paths.captures);
      const existing = yield* fs.readDirectory(paths.captures).pipe(Effect.orElseSucceed(() => []));
      const images = existing.filter((file) => file.endsWith(".png")).sort();
      yield* Ref.set(
        byId,
        new Map(
          images.map((file) => {
            const id = file.slice(0, -".png".length);
            return [id, { id, kind: "image", path: join(paths.captures, file) }] as const;
          }),
        ),
      );
      const newest = images.at(-1);
      if (newest !== undefined) yield* Ref.set(latestId, Option.some(newest.slice(0, -4)));

      const newId = Effect.gen(function* () {
        const now = yield* DateTime.now;
        const suffix = yield* Random.nextIntBetween(0, 36 ** 3);
        // Sortable ids: chronological order == lexical order.
        return `${DateTime.toEpochMillis(now).toString(36)}-${suffix.toString(36).padStart(3, "0")}`;
      });

      const timestampName = (prefix: string) =>
        DateTime.now.pipe(
          Effect.map((now) => {
            const p = DateTime.toParts(DateTime.setZone(now, DateTime.zoneMakeLocal()));
            return `${prefix} ${p.year}-${pad(p.month)}-${pad(p.day)} at ${pad(p.hour)}.${pad(p.minute)}.${pad(p.second)}`;
          }),
        );

      const prune = Effect.gen(function* () {
        const files = yield* fs.readDirectory(paths.captures);
        const stale = files
          .filter((file) => file.endsWith(".png"))
          .sort()
          .slice(0, -KEEP_IMAGES);
        yield* Effect.forEach(stale, (file) => {
          const id = file.slice(0, -4);
          return Effect.all([
            fs.remove(join(paths.captures, file), { force: true }),
            fs.remove(join(paths.captures, `${id}.ocr.json`), { force: true }),
            Ref.update(byId, (map) => {
              const next = new Map(map);
              next.delete(id);
              return next;
            }),
          ]);
        });
      }).pipe(Effect.ignore);

      const newImage = Effect.gen(function* () {
        const id = yield* newId;
        return { id, kind: "image", path: join(paths.captures, `${id}.png`) } satisfies Capture;
      });

      const newRecording = Effect.fn("CaptureStore.newRecording")(function* (directory: string) {
        yield* ensureDir(directory);
        const id = yield* newId;
        const name = yield* timestampName("Recording");
        return { id, kind: "video", path: join(directory, `${name}.mov`) } satisfies Capture;
      });

      const register = Effect.fn("CaptureStore.register")(function* (capture: Capture) {
        yield* Ref.update(byId, (map) => new Map(map).set(capture.id, capture));
        yield* Ref.set(latestId, Option.some(capture.id));
        if (capture.kind === "image") yield* Effect.forkDetach(prune);
      });

      const get = Effect.fn("CaptureStore.get")(function* (id: CaptureId) {
        const capture = (yield* Ref.get(byId)).get(id);
        if (capture === undefined) return yield* new CaptureNotFound({ id });
        return capture;
      });

      const latest = Effect.gen(function* () {
        const id = yield* Ref.get(latestId);
        const map = yield* Ref.get(byId);
        return Option.flatMap(id, (value) => Option.fromNullishOr(map.get(value)));
      });

      return CaptureStore.of({
        newImage,
        newRecording,
        register,
        get,
        latest,
        timestampName,
      });
    }),
  );
}
