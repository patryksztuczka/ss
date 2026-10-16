const SYMBOLS: Record<string, string> = {
  Command: "⌘",
  CommandOrControl: "⌘",
  Control: "⌃",
  Alt: "⌥",
  Option: "⌥",
  Shift: "⇧",
  Space: "Space",
  Return: "↩",
  Enter: "↩",
  Backspace: "⌫",
  Delete: "⌦",
  Escape: "⎋",
  Up: "↑",
  Down: "↓",
  Left: "←",
  Right: "→",
  Plus: "+",
};

/** "Alt+Shift+4" → ["⌥", "⇧", "4"] (macOS menu order: ⌃⌥⇧⌘). */
export const acceleratorParts = (accelerator: string): Array<string> => {
  if (accelerator === "") return [];
  const parts = accelerator.split("+");
  const key = parts.at(-1) ?? "";
  const order = ["Control", "Alt", "Option", "Shift", "Command", "CommandOrControl"];
  const modifiers = parts
    .slice(0, -1)
    .sort((a, b) => order.indexOf(a) - order.indexOf(b))
    .map((modifier) => SYMBOLS[modifier] ?? modifier);
  return [...modifiers, SYMBOLS[key] ?? key];
};

/** "Alt+Shift+4" → "⌥⇧4". */
export const formatAccelerator = (accelerator: string): string =>
  accelerator === "" ? "—" : acceleratorParts(accelerator).join("");

const KEY_BY_CODE: Record<string, string> = {
  Space: "Space",
  Enter: "Return",
  Backspace: "Backspace",
  Delete: "Delete",
  Tab: "Tab",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backquote: "`",
};

/**
 * Builds an Electron accelerator from a keydown. Uses `event.code` so ⌥ combinations
 * (which produce special characters in `event.key`) still map to the physical key.
 * Returns `null` while only modifiers are held.
 */
export const acceleratorFromEvent = (event: KeyboardEvent): string | null => {
  const { code } = event;
  let key: string | undefined;
  if (code.startsWith("Key")) key = code.slice(3);
  else if (code.startsWith("Digit")) key = code.slice(5);
  else if (/^F\d{1,2}$/.test(code)) key = code;
  else key = KEY_BY_CODE[code];
  if (key === undefined) return null;

  const modifiers = [
    event.ctrlKey && "Control",
    event.altKey && "Alt",
    event.shiftKey && "Shift",
    event.metaKey && "Command",
  ].filter((value): value is string => typeof value === "string");
  // Bare keys would hijack typing everywhere; require a modifier unless it's a function key.
  if (modifiers.length === 0 && !/^F\d+$/.test(key)) return null;
  return [...modifiers, key].join("+");
};
