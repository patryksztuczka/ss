import { Effect, Schema } from "effect";

import type { EditorStyle } from "../../../shared/settings";
import { backgroundById, paintBackground } from "./backgrounds";

export class ComposeError extends Schema.TaggedError<ComposeError>()("ComposeError", {
  message: Schema.String,
}) {}

export interface Box {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

/** Everything in output pixels at 1:1 with the source screenshot. */
export interface Layout {
  readonly width: number;
  readonly height: number;
  /** The rounded "card": screenshot + inset. */
  readonly card: Box;
  /** Where the screenshot itself lands. */
  readonly image: Box;
  readonly radius: number;
  readonly inset: number;
}

const RATIOS = { "16:9": 16 / 9, "4:3": 4 / 3, "3:2": 3 / 2, "1:1": 1 } as const;

/**
 * Sizes are relative to the screenshot (geometric mean of its sides), so a style looks the same
 * on a tiny selection and on a 5K full-screen capture.
 */
export const computeLayout = (
  style: EditorStyle,
  imageWidth: number,
  imageHeight: number,
): Layout => {
  const base = Math.sqrt(imageWidth * imageHeight);
  const padding = Math.round((style.padding / 100) * 0.24 * base);
  const inset = Math.round((style.inset / 100) * 0.1 * base);
  const radius = (style.radius / 100) * 0.045 * base;

  const cardWidth = imageWidth + inset * 2;
  const cardHeight = imageHeight + inset * 2;
  let width = cardWidth + padding * 2;
  let height = cardHeight + padding * 2;
  if (style.ratio !== "auto") {
    const ratio = RATIOS[style.ratio];
    if (width / height < ratio) width = Math.round(height * ratio);
    else height = Math.round(width / ratio);
  }
  const card = {
    x: Math.round((width - cardWidth) / 2),
    y: Math.round((height - cardHeight) / 2),
    width: cardWidth,
    height: cardHeight,
  };
  return {
    width,
    height,
    card,
    image: { x: card.x + inset, y: card.y + inset, width: imageWidth, height: imageHeight },
    radius,
    inset,
  };
};

const roundedRect = (
  ctx: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D,
  box: Box,
  radius: number,
) => {
  ctx.beginPath();
  ctx.roundRect(
    box.x,
    box.y,
    box.width,
    box.height,
    Math.min(radius, box.width / 2, box.height / 2),
  );
};

/**
 * Paints the composition into `ctx` at `scale` (1 = full resolution export; smaller for the
 * on-screen preview, which keeps big blurs cheap while dragging sliders).
 */
export const renderComposition = (
  ctx: CanvasRenderingContext2D,
  source: CanvasImageSource,
  style: EditorStyle,
  layout: Layout,
  insetColor: string,
  scale: number,
) => {
  const width = Math.max(1, Math.round(layout.width * scale));
  const height = Math.max(1, Math.round(layout.height * scale));
  if (ctx.canvas.width !== width) ctx.canvas.width = width;
  if (ctx.canvas.height !== height) ctx.canvas.height = height;

  ctx.save();
  ctx.imageSmoothingQuality = "high";
  paintBackground(ctx, width, height, backgroundById(style.background), style.customColor);
  ctx.scale(scale, scale);

  // Build the card off-screen first so the drop shadow follows its exact (alpha) silhouette.
  const card = new OffscreenCanvas(
    Math.max(1, Math.ceil(layout.card.width * scale)),
    Math.max(1, Math.ceil(layout.card.height * scale)),
  );
  const cardCtx = card.getContext("2d");
  if (cardCtx === null) return ctx.restore();
  cardCtx.scale(scale, scale);
  const local = { x: 0, y: 0, width: layout.card.width, height: layout.card.height };
  roundedRect(cardCtx, local, layout.radius);
  cardCtx.clip();
  if (layout.inset > 0) {
    cardCtx.fillStyle = insetColor;
    cardCtx.fillRect(0, 0, local.width, local.height);
  }
  cardCtx.imageSmoothingQuality = "high";
  cardCtx.drawImage(source, layout.inset, layout.inset, layout.image.width, layout.image.height);

  const strength = style.shadow / 100;
  if (strength > 0) {
    const base = Math.sqrt(layout.image.width * layout.image.height);
    ctx.shadowColor = `rgba(15, 20, 40, ${0.12 + strength * 0.33})`;
    ctx.shadowBlur = strength * 0.07 * base * scale;
    ctx.shadowOffsetY = strength * 0.022 * base * scale;
  }
  ctx.drawImage(card, layout.card.x, layout.card.y, layout.card.width, layout.card.height);
  ctx.restore();
};

/** Most common color along the screenshot's border — used to fill the inset seamlessly. */
export const detectEdgeColor = (source: ImageBitmap): string => {
  const size = 64;
  const canvas = new OffscreenCanvas(size, size);
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (ctx === null) return "#ffffff";
  ctx.drawImage(source, 0, 0, size, size);
  const { data } = ctx.getImageData(0, 0, size, size);
  const counts = new Map<number, number>();
  for (let i = 0; i < size; i++) {
    for (const [x, y] of [
      [i, 0],
      [i, size - 1],
      [0, i],
      [size - 1, i],
    ] as const) {
      const offset = (y * size + x) * 4;
      // Quantize to 4 bits per channel so near-identical pixels vote together.
      const key =
        ((data[offset]! >> 4) << 8) | ((data[offset + 1]! >> 4) << 4) | (data[offset + 2]! >> 4);
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
  }
  let best = 0xfff;
  let bestCount = -1;
  for (const [key, count] of counts) {
    if (count > bestCount) [best, bestCount] = [key, count];
  }
  const channel = (shift: number) => (((best >> shift) & 15) * 17).toString(16).padStart(2, "0");
  return `#${channel(8)}${channel(4)}${channel(0)}`;
};

export const loadBitmap = (png: Uint8Array) =>
  Effect.tryPromise({
    try: () => createImageBitmap(new Blob([new Uint8Array(png)], { type: "image/png" })),
    catch: () => new ComposeError({ message: "Cannot decode screenshot" }),
  });

/** Full-resolution PNG of the composition. */
export const exportPng = Effect.fn("exportPng")(function* (
  source: ImageBitmap,
  style: EditorStyle,
  insetColor: string,
) {
  const layout = computeLayout(style, source.width, source.height);
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d");
  if (ctx === null) return yield* new ComposeError({ message: "Canvas unavailable" });
  renderComposition(ctx, source, style, layout, insetColor, 1);
  const blob = yield* Effect.callback<Blob, ComposeError>((resume) => {
    canvas.toBlob(
      (result) =>
        resume(
          result === null
            ? Effect.fail(new ComposeError({ message: "Export failed" }))
            : Effect.succeed(result),
        ),
      "image/png",
    );
  });
  const buffer = yield* Effect.promise(() => blob.arrayBuffer());
  return new Uint8Array(buffer);
});
