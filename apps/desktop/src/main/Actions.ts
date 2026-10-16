import { nativeImage, Notification, shell } from "electron";
import { Context, Effect, FileSystem, Layer, Option, Semaphore } from "effect";

import type { CaptureId, PreviewPayload } from "../shared/ipc";
import { CaptureStore, type Capture } from "./CaptureStore";
import { Clipboard } from "./Clipboard";
import { CaptureNotFound, OcrError } from "./errors";
import { Ocr } from "./Ocr";
import { Paths } from "./Paths";
import { Recorder } from "./Recorder";
import { Screencapture } from "./Screencapture";
import { SettingsStore } from "./SettingsStore";
import { displayUnderCursor, Windows } from "./Windows";

/** Everything a shortcut, the tray menu or a window can trigger. */
export class Actions extends Context.Service<
  Actions,
  {
    readonly captureArea: Effect.Effect<void>;
    readonly captureScreen: Effect.Effect<void>;
    readonly recordArea: Effect.Effect<void>;
    readonly recordScreen: Effect.Effect<void>;
    readonly stopRecording: Effect.Effect<void>;
    readonly openLast: Effect.Effect<void>;
    readonly openSettings: Effect.Effect<void>;
    readonly openOnboarding: Effect.Effect<void>;
    openFolder(kind: "images" | "recordings"): Effect.Effect<void>;
    openEditor(id: CaptureId): Effect.Effect<void, CaptureNotFound>;
    reveal(id: CaptureId): Effect.Effect<void, CaptureNotFound>;
    copyText(id: CaptureId): Effect.Effect<number, OcrError | CaptureNotFound>;
  }
>()("ss/main/Actions") {
  static readonly layer = Layer.effect(
    Actions,
    Effect.gen(function* () {
      const screencapture = yield* Screencapture;
      const store = yield* CaptureStore;
      const clipboard = yield* Clipboard;
      const ocr = yield* Ocr;
      const recorder = yield* Recorder;
      const settings = yield* SettingsStore;
      const windows = yield* Windows;
      const paths = yield* Paths;
      const fs = yield* FileSystem.FileSystem;
      // One capture at a time; repeated shortcut presses while selecting are ignored.
      const busy = yield* Semaphore.make(1);

      const notify = (title: string, body: string) =>
        Effect.sync(() => new Notification({ title, body, silent: true }).show());

      // Missing Screen Recording permission → the setup guide explains and fixes it.
      const askForPermission = windows.openOnboarding;

      const reportFailure = <A, E extends { readonly _tag: string; readonly message?: string }, R>(
        self: Effect.Effect<A, E, R>,
      ) =>
        self.pipe(
          Effect.asVoid,
          Effect.catch((error) =>
            error._tag === "ScreenPermissionError"
              ? askForPermission
              : Effect.logError(error).pipe(
                  Effect.andThen(notify("ss", error.message ?? error._tag)),
                ),
          ),
        );

      const previewPayload = Effect.fn("Actions.previewPayload")(function* (capture: Capture) {
        const size = capture.size ?? nativeImage.createFromPath(capture.path).getSize();
        const thumbnail = yield* windows.thumbnail(capture.path, size);
        return {
          id: capture.id,
          kind: capture.kind,
          thumbnail: thumbnail.isEmpty() ? "" : thumbnail.toDataURL(),
          width: size.width,
          height: size.height,
        } satisfies PreviewPayload;
      });

      const afterImage = Effect.fn("Actions.afterImage")(function* (capture: Capture) {
        yield* store.register(capture);
        yield* clipboard.imageFromFile(capture.path);
        // Warm up OCR so text is selectable by the time the editor opens.
        yield* ocr.recognize(capture.id).pipe(Effect.ignore, Effect.forkDetach);
        const { afterCapture } = yield* settings.get;
        if (afterCapture === "editor") return yield* windows.openEditor(capture.id);
        if (afterCapture === "preview") {
          yield* windows.showPreview(yield* previewPayload(capture));
        }
      });

      const afterRecording = (capture: Capture) =>
        Effect.gen(function* () {
          yield* clipboard.file(capture.path);
          yield* windows.showPreview(yield* previewPayload(capture));
        }).pipe(reportFailure);

      const exclusive = <E extends { readonly _tag: string; readonly message?: string }>(
        self: Effect.Effect<void, E>,
      ) =>
        busy
          .withPermitsIfAvailable(1)(windows.hidePreview.pipe(Effect.andThen(self)))
          .pipe(reportFailure);

      const captureArea = exclusive(
        Effect.gen(function* () {
          const capture = yield* store.newImage;
          const result = yield* screencapture.interactive(capture.path);
          if (Option.isSome(result)) yield* afterImage(capture);
        }),
      ).pipe(Effect.withSpan("Actions.captureArea"));

      const captureScreen = exclusive(
        Effect.gen(function* () {
          const capture = yield* store.newImage;
          yield* screencapture.rect(capture.path, displayUnderCursor().bounds);
          yield* afterImage(capture);
        }),
      ).pipe(Effect.withSpan("Actions.captureScreen"));

      const toggleRecording = (pickArea: boolean) =>
        Effect.gen(function* () {
          const state = yield* recorder.state;
          if (state._tag === "Recording") return yield* recorder.stop;
          yield* exclusive(
            Effect.gen(function* () {
              const selection = pickArea
                ? yield* windows.selectRegion
                : Option.some({ rect: displayUnderCursor().bounds });
              if (Option.isNone(selection)) return;
              yield* recorder.start(selection.value.rect, afterRecording);
            }),
          );
        });

      const openEditor = Effect.fn("Actions.openEditor")(function* (id: CaptureId) {
        const capture = yield* store.get(id);
        yield* windows.hidePreview;
        if (capture.kind === "image") return yield* windows.openEditor(id);
        shell.showItemInFolder(capture.path);
      });

      const reveal = Effect.fn("Actions.reveal")(function* (id: CaptureId) {
        const capture = yield* store.get(id);
        shell.showItemInFolder(capture.path);
      });

      const openLast = Effect.gen(function* () {
        const latest = yield* store.latest;
        if (Option.isNone(latest)) return yield* notify("ss", "Nothing captured yet");
        yield* openEditor(latest.value.id);
      }).pipe(reportFailure);

      const copyText = Effect.fn("Actions.copyText")(function* (id: CaptureId) {
        const layout = yield* ocr.recognize(id);
        const text = layout.lines.map((line) => line.text).join("\n");
        if (text !== "") yield* clipboard.text(text);
        return layout.lines.length;
      });

      return Actions.of({
        captureArea,
        captureScreen,
        recordArea: toggleRecording(true),
        recordScreen: toggleRecording(false),
        stopRecording: recorder.stop,
        openLast,
        openSettings: windows.openSettings,
        openOnboarding: windows.openOnboarding,
        openFolder: (kind) =>
          Effect.gen(function* () {
            const current = yield* settings.get;
            const directory =
              kind === "images"
                ? (current.imagesDirectory ?? paths.defaultImages)
                : (current.recordingsDirectory ?? paths.defaultRecordings);
            yield* fs.makeDirectory(directory, { recursive: true });
            yield* Effect.promise(() => shell.openPath(directory));
          }).pipe(reportFailure),
        openEditor,
        reveal,
        copyText,
      });
    }),
  );
}
