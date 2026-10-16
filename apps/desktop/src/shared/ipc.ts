import { Schema } from "effect";

import { TextLayout } from "./ocr";
import { Settings, SettingsPatch, ShortcutAction } from "./settings";

export const CaptureId = Schema.String.check(Schema.isPattern(/^[A-Za-z0-9_-]+$/));
export type CaptureId = typeof CaptureId.Type;

export const CaptureKind = Schema.Literals(["image", "video"]);
export type CaptureKind = typeof CaptureKind.Type;

export const CaptureImage = Schema.Struct({
  id: CaptureId,
  width: Schema.Number,
  height: Schema.Number,
  png: Schema.Uint8Array,
});
export type CaptureImage = typeof CaptureImage.Type;

export const PreviewPayload = Schema.Struct({
  id: CaptureId,
  kind: CaptureKind,
  /** data: URL of a small thumbnail */
  thumbnail: Schema.String,
  width: Schema.Number,
  height: Schema.Number,
});
export type PreviewPayload = typeof PreviewPayload.Type;

export const Rect = Schema.Struct({
  x: Schema.Number,
  y: Schema.Number,
  width: Schema.Number,
  height: Schema.Number,
});
export type Rect = typeof Rect.Type;

export const SettingsUpdateResult = Schema.Struct({
  settings: Settings,
  failedShortcuts: Schema.Array(ShortcutAction),
});
export type SettingsUpdateResult = typeof SettingsUpdateResult.Type;

export const AppInfo = Schema.Struct({
  version: Schema.String,
  defaultImagesDirectory: Schema.String,
  defaultRecordingsDirectory: Schema.String,
});
export type AppInfo = typeof AppInfo.Type;

export const ScreenAccess = Schema.Literals(["granted", "denied"]);
export type ScreenAccess = typeof ScreenAccess.Type;

/** Everything the setup guide and Settings need to show permission / integration status. */
export const SetupState = Schema.Struct({
  screenAccess: ScreenAccess,
  /** Whether macOS still owns ⇧⌘3 / ⇧⌘4 / ⇧⌘5. */
  systemScreenshotShortcuts: Schema.Boolean,
  launchAtLogin: Schema.Struct({ available: Schema.Boolean, enabled: Schema.Boolean }),
  /** In development the terminal running ss is the app macOS asks about. */
  permissionHost: Schema.NullOr(Schema.String),
});
export type SetupState = typeof SetupState.Type;

const ById = Schema.Struct({ id: CaptureId });

const contract = <I extends Schema.Top, O extends Schema.Top>(input: I, output: O) => ({
  input,
  output,
});

/** Renderer → main request/response channels. Both sides validate with these schemas. */
export const Invoke = {
  "app:info": contract(Schema.Void, AppInfo),
  "settings:get": contract(Schema.Void, Settings),
  "settings:update": contract(SettingsPatch, SettingsUpdateResult),
  "settings:pick-directory": contract(
    Schema.Struct({ kind: Schema.Literals(["images", "recordings"]) }),
    Schema.NullOr(Schema.String),
  ),
  "setup:state": contract(Schema.Void, SetupState),
  "setup:request-screen-access": contract(Schema.Void, SetupState),
  "setup:apply-shortcuts": contract(
    Schema.Struct({ preset: Schema.Literals(["system", "alongside"]) }),
    SettingsUpdateResult,
  ),
  "setup:launch-at-login": contract(Schema.Struct({ enabled: Schema.Boolean }), SetupState),
  "setup:open": contract(Schema.Void, Schema.Void),
  "setup:finish": contract(Schema.Void, Schema.Void),
  "shortcuts:pause": contract(Schema.Struct({ paused: Schema.Boolean }), Schema.Void),
  "capture:load": contract(ById, CaptureImage),
  "capture:ocr": contract(ById, TextLayout),
  "clipboard:write-image": contract(Schema.Struct({ png: Schema.Uint8Array }), Schema.Void),
  "clipboard:write-text": contract(Schema.Struct({ text: Schema.String }), Schema.Void),
  "image:save": contract(
    Schema.Struct({ png: Schema.Uint8Array, mode: Schema.Literals(["quick", "dialog"]) }),
    Schema.NullOr(Schema.String),
  ),
  "preview:edit": contract(ById, Schema.Void),
  "preview:reveal": contract(ById, Schema.Void),
  "preview:copy-text": contract(ById, Schema.Number),
  "preview:drag": contract(ById, Schema.Void),
  "preview:dismiss": contract(Schema.Void, Schema.Void),
  "region:submit": contract(Schema.Struct({ rect: Schema.NullOr(Rect) }), Schema.Void),
  "region:cancel": contract(Schema.Void, Schema.Void),
  "window:close": contract(Schema.Void, Schema.Void),
} as const;

export type InvokeChannel = keyof typeof Invoke;
export type InvokeInput<C extends InvokeChannel> = (typeof Invoke)[C]["input"]["Type"];
export type InvokeOutput<C extends InvokeChannel> = (typeof Invoke)[C]["output"]["Type"];

/** Main → renderer push channels. */
export const Events = {
  "preview:show": PreviewPayload,
  "settings:changed": Settings,
} as const;

export type EventChannel = keyof typeof Events;
export type EventPayload<C extends EventChannel> = (typeof Events)[C]["Type"];

/** IPC result envelope, so typed failures survive the process boundary. */
export const IpcResult = Schema.Union([
  Schema.Struct({ ok: Schema.Literal(true), value: Schema.Unknown }),
  Schema.Struct({ ok: Schema.Literal(false), error: Schema.String }),
]);
export type IpcResult = typeof IpcResult.Type;

export const invokeChannels = Object.keys(Invoke) as ReadonlyArray<InvokeChannel>;
export const eventChannels = Object.keys(Events) as ReadonlyArray<EventChannel>;
