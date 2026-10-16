import { join } from "node:path";
import {
  app,
  BrowserWindow,
  dialog,
  ipcMain,
  nativeImage,
  type IpcMainInvokeEvent,
} from "electron";
import { Effect, FileSystem, Layer, Schema } from "effect";

import { shortcutPresets } from "../shared/settings";
import {
  Invoke,
  invokeChannels,
  type InvokeChannel,
  type InvokeInput,
  type InvokeOutput,
  type IpcResult,
} from "../shared/ipc";
import { Actions } from "./Actions";
import { CaptureStore } from "./CaptureStore";
import { Clipboard } from "./Clipboard";
import { CaptureNotFound, StorageError } from "./errors";
import { Ocr } from "./Ocr";
import { Paths } from "./Paths";
import { SettingsStore } from "./SettingsStore";
import { Shortcuts } from "./Shortcuts";
import { System } from "./System";
import { Windows } from "./Windows";

interface Failure {
  readonly _tag: string;
  readonly message?: string;
}

type Handlers = {
  readonly [C in InvokeChannel]: (
    input: InvokeInput<C>,
    event: IpcMainInvokeEvent,
  ) => Effect.Effect<InvokeOutput<C>, Failure>;
};

const senderWindow = (event: IpcMainInvokeEvent) =>
  BrowserWindow.fromWebContents(event.sender) ?? undefined;

/** Wires every channel of the shared contract to an Effect handler, validating input with Schema. */
export const IpcLive = Layer.effectDiscard(
  Effect.gen(function* () {
    const actions = yield* Actions;
    const store = yield* CaptureStore;
    const clipboard = yield* Clipboard;
    const ocr = yield* Ocr;
    const paths = yield* Paths;
    const settings = yield* SettingsStore;
    const shortcuts = yield* Shortcuts;
    const windows = yield* Windows;
    const system = yield* System;
    const fs = yield* FileSystem.FileSystem;
    const context = yield* Effect.context<never>();

    const writeFile = (path: string, data: Uint8Array) =>
      fs
        .writeFile(path, data)
        .pipe(
          Effect.mapError((cause) => new StorageError({ message: `Cannot write ${path}`, cause })),
        );

    const handlers: Handlers = {
      "app:info": () =>
        Effect.succeed({
          version: app.getVersion(),
          defaultImagesDirectory: paths.defaultImages,
          defaultRecordingsDirectory: paths.defaultRecordings,
        }),
      "settings:get": () => settings.get,
      "settings:update": (patch) =>
        Effect.gen(function* () {
          const next = yield* settings.update(patch);
          const failedShortcuts =
            patch.shortcuts === undefined ? [] : yield* shortcuts.apply(patch.shortcuts);
          yield* windows.broadcast("settings:changed", next);
          return { settings: next, failedShortcuts };
        }),
      "settings:pick-directory": ({ kind }, event) =>
        Effect.gen(function* () {
          const window = senderWindow(event);
          const options: Electron.OpenDialogOptions = {
            properties: ["openDirectory", "createDirectory"],
            buttonLabel: "Choose",
          };
          const result = yield* Effect.promise(() =>
            window ? dialog.showOpenDialog(window, options) : dialog.showOpenDialog(options),
          );
          const directory = result.filePaths[0];
          if (result.canceled || directory === undefined) return null;
          const next = yield* settings.update(
            kind === "images" ? { imagesDirectory: directory } : { recordingsDirectory: directory },
          );
          yield* windows.broadcast("settings:changed", next);
          return directory;
        }),
      "setup:state": () => system.state,
      "setup:request-screen-access": () =>
        system.requestScreenAccess.pipe(Effect.andThen(system.state)),
      "setup:apply-shortcuts": ({ preset }) =>
        Effect.gen(function* () {
          // macOS must release ⇧⌘3/4/5 before ss can claim them (and get them back otherwise).
          yield* system.setSystemScreenshotShortcuts(preset === "alongside");
          const next = yield* settings.update({ shortcuts: shortcutPresets[preset] });
          const failedShortcuts = yield* shortcuts.apply(next.shortcuts);
          yield* windows.broadcast("settings:changed", next);
          return { settings: next, failedShortcuts };
        }),
      "setup:launch-at-login": ({ enabled }) =>
        system.setLaunchAtLogin(enabled).pipe(Effect.andThen(system.state)),
      "setup:open": () => windows.openOnboarding,
      "setup:finish": () =>
        settings.update({ onboarded: true }).pipe(
          Effect.tap((next) => windows.broadcast("settings:changed", next)),
          Effect.andThen(windows.closeOnboarding),
        ),
      "shortcuts:pause": ({ paused }) => shortcuts.setPaused(paused),
      "capture:load": ({ id }) =>
        Effect.gen(function* () {
          const capture = yield* store.get(id);
          if (capture.kind !== "image") return yield* new CaptureNotFound({ id });
          const png = yield* fs
            .readFile(capture.path)
            .pipe(
              Effect.mapError(
                (cause) => new StorageError({ message: "Cannot read capture", cause }),
              ),
            );
          const { width, height } = nativeImage.createFromPath(capture.path).getSize();
          return { id, width, height, png };
        }),
      "capture:ocr": ({ id }) => ocr.recognize(id),
      "clipboard:write-image": ({ png }) => clipboard.imageFromPng(png),
      "clipboard:write-text": ({ text }) => clipboard.text(text),
      "image:save": ({ png, mode }, event) =>
        Effect.gen(function* () {
          const directory = (yield* settings.get).imagesDirectory ?? paths.defaultImages;
          yield* fs
            .makeDirectory(directory, { recursive: true })
            .pipe(
              Effect.mapError(
                (cause) => new StorageError({ message: `Cannot create ${directory}`, cause }),
              ),
            );
          const defaultPath = join(directory, `${yield* store.timestampName("ss")}.png`);
          if (mode === "quick") {
            yield* writeFile(defaultPath, png);
            return defaultPath;
          }
          const window = senderWindow(event);
          const options: Electron.SaveDialogOptions = {
            defaultPath,
            filters: [{ name: "PNG Image", extensions: ["png"] }],
          };
          const result = yield* Effect.promise(() =>
            window ? dialog.showSaveDialog(window, options) : dialog.showSaveDialog(options),
          );
          if (result.canceled || result.filePath === undefined) return null;
          yield* writeFile(result.filePath, png);
          return result.filePath;
        }),
      "preview:edit": ({ id }) => actions.openEditor(id),
      "preview:reveal": ({ id }) => actions.reveal(id),
      "preview:copy-text": ({ id }) => actions.copyText(id),
      "preview:drag": ({ id }, event) =>
        Effect.gen(function* () {
          const capture = yield* store.get(id);
          const size = capture.size ?? nativeImage.createFromPath(capture.path).getSize();
          const icon = yield* windows.thumbnail(capture.path, size, 160);
          event.sender.startDrag({ file: capture.path, icon });
        }),
      "preview:dismiss": () => windows.hidePreview,
      "region:submit": ({ rect }) => windows.resolveRegion(rect),
      "region:cancel": () => windows.cancelRegion,
      "window:close": (_, event) => Effect.sync(() => senderWindow(event)?.close()),
    };

    const register = <C extends InvokeChannel>(channel: C) => {
      const decode = Schema.decodeUnknownEffect(
        Invoke[channel].input as Schema.Codec<InvokeInput<C>>,
      );
      const handler = handlers[channel] as Handlers[C];
      ipcMain.handle(`ss:${channel}`, (event, raw: unknown) =>
        Effect.runPromiseWith(context)(
          decode(raw).pipe(
            Effect.mapError((issue): Failure => ({ _tag: "InvalidInput", message: issue.message })),
            Effect.flatMap((input) => handler(input, event)),
            Effect.map((value): IpcResult => ({ ok: true, value })),
            Effect.catch((error) =>
              Effect.logError(`ipc ${channel} failed`, error).pipe(
                Effect.as<IpcResult>({ ok: false, error: error.message ?? error._tag }),
              ),
            ),
            Effect.withSpan(`ipc.${channel}`),
          ),
        ),
      );
    };

    yield* Effect.acquireRelease(
      Effect.sync(() => invokeChannels.forEach(register)),
      () =>
        Effect.sync(() =>
          invokeChannels.forEach((channel) => ipcMain.removeHandler(`ss:${channel}`)),
        ),
    );
  }),
);
