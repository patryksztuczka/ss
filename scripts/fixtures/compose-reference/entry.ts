// Runs the Electron editor's real compositor (`compose.ts`, `backgrounds.ts`)
// in a hidden window and returns reference PNGs. Bundled by `run.mjs`.
import type { EditorStyle } from "../../../apps/desktop/src/shared/settings";
import {
  computeLayout,
  detectEdgeColor,
  renderComposition,
} from "../../../apps/desktop/src/renderer/src/lib/compose";

const toBase64 = async (blob: Blob) => {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
};

const toPng = (canvas: HTMLCanvasElement) =>
  new Promise<Blob>((resolve, reject) =>
    canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error("toBlob"))), "image/png"),
  );

(window as unknown as Record<string, unknown>).renderReference = async (
  pngBase64: string,
  styles: ReadonlyArray<EditorStyle>,
) => {
  const bytes = Uint8Array.from(atob(pngBase64), (char) => char.charCodeAt(0));
  // Same decode as `loadBitmap`.
  const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
  const insetColor = detectEdgeColor(bitmap);

  // The decoded, colour-managed source as the canvas sees it.
  const source = document.createElement("canvas");
  source.width = bitmap.width;
  source.height = bitmap.height;
  source.getContext("2d")!.drawImage(bitmap, 0, 0);

  const outputs = [];
  for (const style of styles) {
    // Exactly `exportPng`: computeLayout + renderComposition at scale 1 + toBlob.
    const layout = computeLayout(style, bitmap.width, bitmap.height);
    const canvas = document.createElement("canvas");
    renderComposition(canvas.getContext("2d")!, bitmap, style, layout, insetColor, 1);
    outputs.push({ style, layout, png: await toBase64(await toPng(canvas)) });
  }
  return { insetColor, source: await toBase64(await toPng(source)), outputs };
};
