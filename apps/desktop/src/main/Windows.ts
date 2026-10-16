import { app, BrowserWindow, nativeImage, screen, shell, type Display } from "electron";
import { Context, Deferred, Effect, Layer, Option, Ref } from "effect";

import type { CaptureId, EventChannel, EventPayload, PreviewPayload, Rect } from "../shared/ipc";
import { Paths } from "./Paths";

const APP_BACKGROUND = "#f7f5f2";
const PREVIEW_WIDTH = 248;
const PREVIEW_MARGIN = 18; // transparent gutter for the drop shadow + slide animation

export interface RegionSelection {
  /** Global screen points. */
  readonly rect: Rect;
  readonly display: Display;
}

export const displayUnderCursor = () =>
  screen.getDisplayNearestPoint(screen.getCursorScreenPoint());

export class Windows extends Context.Service<
  Windows,
  {
    showPreview(payload: PreviewPayload): Effect.Effect<void>;
    readonly hidePreview: Effect.Effect<void>;
    /** Aspect-correct thumbnail, `width` px wide (2× the preview card for Retina). */
    thumbnail(
      path: string,
      size: { readonly width: number; readonly height: number },
      width?: number,
    ): Effect.Effect<Electron.NativeImage>;
    openEditor(id: CaptureId): Effect.Effect<void>;
    readonly openSettings: Effect.Effect<void>;
    readonly openOnboarding: Effect.Effect<void>;
    readonly closeOnboarding: Effect.Effect<void>;
    /** Shows a dimmed overlay on the active display and waits for a drag selection. */
    readonly selectRegion: Effect.Effect<Option.Option<RegionSelection>>;
    resolveRegion(localRect: Rect | null): Effect.Effect<void>;
    readonly cancelRegion: Effect.Effect<void>;
    broadcast<C extends EventChannel>(channel: C, payload: EventPayload<C>): Effect.Effect<void>;
  }
>()("ss/main/Windows") {
  static readonly layer = Layer.effect(
    Windows,
    Effect.gen(function* () {
      const paths = yield* Paths;
      const editors = new Map<CaptureId, BrowserWindow>();
      let preview: BrowserWindow | undefined;
      let settings: BrowserWindow | undefined;
      let onboarding: BrowserWindow | undefined;
      const region = yield* Ref.make(
        Option.none<{
          readonly window: BrowserWindow;
          readonly display: Display;
          readonly result: Deferred.Deferred<Option.Option<RegionSelection>>;
        }>(),
      );

      const load = (window: BrowserWindow, route: string) => {
        window.webContents.setWindowOpenHandler(({ url }) => {
          void shell.openExternal(url);
          return { action: "deny" };
        });
        if (paths.renderer.devServerUrl !== undefined) {
          void window.loadURL(`${paths.renderer.devServerUrl}#${route}`);
        } else {
          void window.loadFile(paths.renderer.indexHtml, { hash: route });
        }
      };

      const webPreferences: Electron.WebPreferences = {
        preload: paths.preload,
        contextIsolation: true,
        nodeIntegration: false,
        sandbox: true,
        spellcheck: false,
      };

      const bringToFront = (window: BrowserWindow) => {
        // ss lives in the menu bar (no Dock icon), so explicitly steal focus for real windows.
        app.focus({ steal: true });
        window.show();
        window.focus();
      };

      const ensurePreview = () => {
        if (preview && !preview.isDestroyed()) return preview;
        const window = new BrowserWindow({
          width: PREVIEW_WIDTH + PREVIEW_MARGIN * 2,
          height: 200,
          show: false,
          frame: false,
          transparent: true,
          hasShadow: false,
          resizable: false,
          movable: false,
          minimizable: false,
          maximizable: false,
          fullscreenable: false,
          focusable: false,
          skipTaskbar: true,
          alwaysOnTop: true,
          webPreferences,
        });
        window.setAlwaysOnTop(true, "floating");
        window.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
        load(window, "/preview");
        preview = window;
        return window;
      };

      const whenLoaded = (window: BrowserWindow) =>
        Effect.callback<void>((resume) => {
          if (!window.webContents.isLoading()) return resume(Effect.void);
          window.webContents.once("did-finish-load", () => resume(Effect.void));
        });

      const showPreview = Effect.fn("Windows.showPreview")(function* (payload: PreviewPayload) {
        const window = ensurePreview();
        yield* whenLoaded(window);
        const aspect = payload.height / Math.max(1, payload.width);
        const thumbHeight = Math.round(Math.min(Math.max(PREVIEW_WIDTH * aspect, 120), 220));
        const { workArea } = displayUnderCursor();
        const width = PREVIEW_WIDTH + PREVIEW_MARGIN * 2;
        const height = thumbHeight + PREVIEW_MARGIN * 2;
        window.setBounds({
          x: workArea.x + workArea.width - width - 6,
          y: workArea.y + workArea.height - height - 6,
          width,
          height,
        });
        window.webContents.send("ss:preview:show", payload);
        window.showInactive();
      });

      const hidePreview = Effect.sync(() => {
        if (preview && !preview.isDestroyed()) preview.hide();
      });

      const thumbnail = (
        path: string,
        size: { readonly width: number; readonly height: number },
        width = PREVIEW_WIDTH * 2,
      ) => {
        const height = Math.max(1, Math.round((width * size.height) / Math.max(1, size.width)));
        const image = nativeImage.createFromPath(path);
        // Images resize directly; movies need QuickLook for a poster frame.
        return image.isEmpty()
          ? Effect.promise(() => nativeImage.createThumbnailFromPath(path, { width, height })).pipe(
              Effect.orElseSucceed(() => nativeImage.createEmpty()),
            )
          : Effect.succeed(image.resize({ width, height, quality: "good" }));
      };

      const openEditor = (id: CaptureId) =>
        Effect.sync(() => {
          const existing = editors.get(id);
          if (existing && !existing.isDestroyed()) return bringToFront(existing);
          const { workArea } = displayUnderCursor();
          const width = Math.min(1240, workArea.width - 80);
          const height = Math.min(800, workArea.height - 80);
          const window = new BrowserWindow({
            width,
            height,
            x: Math.round(workArea.x + (workArea.width - width) / 2),
            y: Math.round(workArea.y + (workArea.height - height) / 2),
            minWidth: 880,
            minHeight: 560,
            show: false,
            title: "ss",
            titleBarStyle: "hiddenInset",
            trafficLightPosition: { x: 18, y: 18 },
            backgroundColor: APP_BACKGROUND,
            webPreferences,
          });
          editors.set(id, window);
          window.on("closed", () => editors.delete(id));
          window.once("ready-to-show", () => bringToFront(window));
          load(window, `/editor?id=${encodeURIComponent(id)}`);
        });

      const openSettings = Effect.sync(() => {
        if (settings && !settings.isDestroyed()) return bringToFront(settings);
        const window = new BrowserWindow({
          width: 560,
          height: 760,
          show: false,
          resizable: false,
          fullscreenable: false,
          minimizable: false,
          title: "ss Settings",
          titleBarStyle: "hiddenInset",
          trafficLightPosition: { x: 18, y: 18 },
          backgroundColor: APP_BACKGROUND,
          webPreferences,
        });
        settings = window;
        window.once("ready-to-show", () => bringToFront(window));
        load(window, "/settings");
      });

      const openOnboarding = Effect.sync(() => {
        if (onboarding && !onboarding.isDestroyed()) return bringToFront(onboarding);
        const window = new BrowserWindow({
          width: 760,
          height: 560,
          show: false,
          resizable: false,
          fullscreenable: false,
          minimizable: false,
          maximizable: false,
          title: "Welcome to ss",
          titleBarStyle: "hiddenInset",
          trafficLightPosition: { x: 18, y: 18 },
          backgroundColor: APP_BACKGROUND,
          webPreferences,
        });
        onboarding = window;
        window.once("ready-to-show", () => bringToFront(window));
        load(window, "/onboarding");
      });

      const closeOnboarding = Effect.sync(() => {
        if (onboarding && !onboarding.isDestroyed()) onboarding.close();
      });

      const finishRegion = (selection: (display: Display) => Option.Option<RegionSelection>) =>
        Ref.getAndSet(region, Option.none()).pipe(
          Effect.flatMap(
            Option.match({
              onNone: () => Effect.void,
              onSome: (active) =>
                Deferred.succeed(active.result, selection(active.display)).pipe(
                  Effect.andThen(
                    Effect.sync(() => {
                      if (!active.window.isDestroyed()) active.window.close();
                    }),
                  ),
                ),
            }),
          ),
        );

      const resolveRegion = (local: Rect | null) =>
        finishRegion((display) => {
          const { bounds } = display;
          const rect =
            local === null
              ? bounds
              : {
                  x: bounds.x + local.x,
                  y: bounds.y + local.y,
                  width: local.width,
                  height: local.height,
                };
          return Option.some({ rect, display });
        });

      const cancelRegion = finishRegion(() => Option.none());

      const selectRegion = Effect.gen(function* () {
        yield* cancelRegion;
        const display = displayUnderCursor();
        const result = yield* Deferred.make<Option.Option<RegionSelection>>();
        const window = new BrowserWindow({
          ...display.bounds,
          show: false,
          frame: false,
          transparent: true,
          hasShadow: false,
          resizable: false,
          movable: false,
          minimizable: false,
          maximizable: false,
          fullscreenable: false,
          enableLargerThanScreen: true,
          skipTaskbar: true,
          alwaysOnTop: true,
          webPreferences,
        });
        window.setAlwaysOnTop(true, "screen-saver");
        window.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
        window.once("ready-to-show", () => {
          window.setBounds(display.bounds);
          bringToFront(window);
        });
        window.on("blur", () => Effect.runFork(cancelRegion));
        window.on("closed", () => Effect.runFork(cancelRegion));
        yield* Ref.set(region, Option.some({ window, display, result }));
        load(window, "/region");
        const selection = yield* Deferred.await(result);
        // Let the overlay fade out before anything is captured.
        yield* Effect.sleep("180 millis");
        return selection;
      });

      const broadcast = <C extends EventChannel>(channel: C, payload: EventPayload<C>) =>
        Effect.sync(() => {
          for (const window of BrowserWindow.getAllWindows()) {
            if (!window.isDestroyed()) window.webContents.send(`ss:${channel}`, payload);
          }
        });

      return Windows.of({
        showPreview,
        hidePreview,
        thumbnail,
        openEditor,
        openSettings,
        openOnboarding,
        closeOnboarding,
        selectRegion,
        resolveRegion,
        cancelRegion,
        broadcast,
      });
    }),
  );
}
