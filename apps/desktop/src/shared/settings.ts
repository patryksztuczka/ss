import { Effect, Schema } from "effect";

const withDefault = <S extends Schema.Top>(value: S["Encoded"]) =>
  Schema.withDecodingDefaultKey<S>(Effect.succeed(value));

export const BackgroundId = Schema.Literals([
  "desktop",
  "cool",
  "nice",
  "morning",
  "bright",
  "love",
  "rain",
  "sky",
  "none",
  "custom",
]);
export type BackgroundId = typeof BackgroundId.Type;

export const AspectRatio = Schema.Literals(["auto", "16:9", "4:3", "3:2", "1:1"]);
export type AspectRatio = typeof AspectRatio.Type;

const Percent = Schema.Number.check(Schema.isBetween({ minimum: 0, maximum: 100 }));

export const EditorStyle = Schema.Struct({
  padding: Percent.pipe(withDefault(36)),
  inset: Percent.pipe(withDefault(0)),
  radius: Percent.pipe(withDefault(30)),
  shadow: Percent.pipe(withDefault(45)),
  background: BackgroundId.pipe(withDefault("sky")),
  customColor: Schema.String.pipe(withDefault("#e8e4df")),
  ratio: AspectRatio.pipe(withDefault("auto")),
});
export type EditorStyle = typeof EditorStyle.Type;

export const ShortcutAction = Schema.Literals([
  "captureArea",
  "captureScreen",
  "recordArea",
  "recordScreen",
  "openLast",
]);
export type ShortcutAction = typeof ShortcutAction.Type;

/** Shortcut sets offered during onboarding. */
export const shortcutPresets = {
  /** Takes over the macOS screenshot keys (requires turning the system ones off). */
  system: {
    captureScreen: "Shift+Command+3",
    captureArea: "Shift+Command+4",
    recordArea: "Shift+Command+5",
    recordScreen: "Alt+Shift+Command+5",
    openLast: "Alt+Shift+Command+4",
  },
  /** Lives next to the macOS ones, which keep working. */
  alongside: {
    captureScreen: "Control+Shift+3",
    captureArea: "Control+Shift+4",
    recordArea: "Control+Shift+5",
    recordScreen: "Control+Alt+Shift+5",
    openLast: "Control+Alt+Shift+4",
  },
} as const satisfies Record<string, Record<ShortcutAction, string>>;
export type ShortcutPreset = keyof typeof shortcutPresets;

/**
 * Since macOS 15, global hotkeys whose only modifiers are ⌥ / ⌥⇧ register fine but never fire.
 * Anything else needs ⌘ or ⌃ (function keys may stand alone).
 */
export const isUsableAccelerator = (accelerator: string): boolean => {
  if (accelerator === "") return true;
  const parts = accelerator.split("+");
  const key = parts.at(-1) ?? "";
  if (/^F\d{1,2}$/.test(key)) return true;
  return parts
    .slice(0, -1)
    .some((modifier) =>
      /^(Command|Cmd|CommandOrControl|CmdOrCtrl|Control|Ctrl|Super|Meta)$/.test(modifier),
    );
};

const defaultShortcuts = shortcutPresets.alongside;

export const Shortcuts = Schema.Struct({
  captureArea: Schema.String.pipe(withDefault(defaultShortcuts.captureArea)),
  captureScreen: Schema.String.pipe(withDefault(defaultShortcuts.captureScreen)),
  recordArea: Schema.String.pipe(withDefault(defaultShortcuts.recordArea)),
  recordScreen: Schema.String.pipe(withDefault(defaultShortcuts.recordScreen)),
  openLast: Schema.String.pipe(withDefault(defaultShortcuts.openLast)),
});
export type Shortcuts = typeof Shortcuts.Type;

/** Replaces shortcuts macOS would silently ignore (e.g. the old ⌥⇧4 defaults). */
export const repairShortcuts = (shortcuts: Shortcuts): Shortcuts => {
  const repaired = { ...shortcuts };
  for (const action of ShortcutAction.literals) {
    if (!isUsableAccelerator(repaired[action])) repaired[action] = defaultShortcuts[action];
  }
  return repaired;
};

export const AfterCapture = Schema.Literals(["preview", "editor", "nothing"]);
export type AfterCapture = typeof AfterCapture.Type;

export const Settings = Schema.Struct({
  shortcuts: Shortcuts.pipe(withDefault({})),
  editor: EditorStyle.pipe(withDefault({})),
  afterCapture: AfterCapture.pipe(withDefault("preview")),
  /** Where ⌘S in the editor writes images. `null` = ~/Pictures/ss */
  imagesDirectory: Schema.NullOr(Schema.String).pipe(withDefault(null)),
  /** Where recordings are written. `null` = ~/Movies/ss */
  recordingsDirectory: Schema.NullOr(Schema.String).pipe(withDefault(null)),
  /** Set once the setup guide has been completed (or skipped). */
  onboarded: Schema.Boolean.pipe(withDefault(false)),
});
export type Settings = typeof Settings.Type;

export const SettingsPatch = Schema.Struct({
  shortcuts: Schema.optionalKey(Shortcuts),
  editor: Schema.optionalKey(EditorStyle),
  afterCapture: Schema.optionalKey(AfterCapture),
  imagesDirectory: Schema.optionalKey(Schema.NullOr(Schema.String)),
  recordingsDirectory: Schema.optionalKey(Schema.NullOr(Schema.String)),
  onboarded: Schema.optionalKey(Schema.Boolean),
});
export type SettingsPatch = typeof SettingsPatch.Type;

export const decodeSettings = Schema.decodeUnknownSync(Settings);
export const defaultSettings: Settings = decodeSettings({});
