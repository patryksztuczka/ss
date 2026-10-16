import { join } from "node:path";
import { app } from "electron";
import { Context, Effect, Layer } from "effect";

export class Paths extends Context.Service<
  Paths,
  {
    /** Throwaway capture history (pruned). Nothing is ever written to the Desktop. */
    readonly captures: string;
    readonly settingsFile: string;
    readonly defaultImages: string;
    readonly defaultRecordings: string;
    readonly helperBinary: string;
    readonly resources: string;
    readonly renderer: { readonly devServerUrl: string | undefined; readonly indexHtml: string };
    readonly preload: string;
  }
>()("ss/main/Paths") {
  static readonly layer = Layer.effect(
    Paths,
    Effect.sync(() => {
      const userData = app.getPath("userData");
      // Executables can't run from inside app.asar; electron-builder unpacks them next to it.
      const unpacked = __dirname.replace("app.asar/", "app.asar.unpacked/");
      return Paths.of({
        captures: join(userData, "captures"),
        settingsFile: join(userData, "settings.json"),
        defaultImages: join(app.getPath("pictures"), "ss"),
        defaultRecordings: join(app.getPath("videos"), "ss"),
        helperBinary: join(unpacked, "bin", "ss-helper"),
        resources: app.isPackaged ? process.resourcesPath : join(__dirname, "..", "resources"),
        renderer: {
          devServerUrl: process.env.VITE_DEV_SERVER_URL,
          indexHtml: join(__dirname, "renderer", "index.html"),
        },
        preload: join(__dirname, "preload.cjs"),
      });
    }),
  );
}
