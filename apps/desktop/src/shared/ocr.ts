import { Schema } from "effect";

/** Normalized (0..1) box with a top-left origin. */
export const TextBox = Schema.Struct({
  text: Schema.String,
  x: Schema.Number,
  y: Schema.Number,
  w: Schema.Number,
  h: Schema.Number,
});
export type TextBox = typeof TextBox.Type;

export const TextLine = Schema.Struct({
  ...TextBox.fields,
  words: Schema.Array(TextBox),
});
export type TextLine = typeof TextLine.Type;

export const TextLayout = Schema.Struct({
  width: Schema.Number,
  height: Schema.Number,
  lines: Schema.Array(TextLine),
});
export type TextLayout = typeof TextLayout.Type;
