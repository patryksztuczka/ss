import { memo, useMemo, type Ref } from "react";

import type { TextBox, TextLayout } from "../../../shared/ocr";
import { cx } from "./ui";

const FONT = `-apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif`;
const measureContext = new OffscreenCanvas(1, 1).getContext("2d");

interface Glyph {
  readonly key: string;
  readonly line: number;
  readonly text: string;
  readonly left: number;
  readonly top: number;
  readonly fontSize: number;
  readonly scaleX: number;
}

const toGlyph = (
  box: TextBox,
  line: number,
  index: number,
  width: number,
  height: number,
): Glyph => {
  const fontSize = Math.max(1, box.h * height * 0.92);
  let natural = box.w * width;
  if (measureContext !== null) {
    measureContext.font = `${fontSize}px ${FONT}`;
    natural = measureContext.measureText(box.text).width || natural;
  }
  return {
    key: `${line}-${index}`,
    line,
    text: box.text,
    left: box.x * width,
    top: box.y * height + box.h * height * 0.04,
    fontSize,
    scaleX: (box.w * width) / natural,
  };
};

/**
 * Invisible, selectable text positioned over the screenshot in image pixel space.
 * The parent scales it to the on-screen size.
 */
export const TextLayer = memo(function TextLayer({
  layout,
  highlight,
  ref,
}: {
  layout: TextLayout;
  highlight: boolean;
  ref?: Ref<HTMLDivElement>;
}) {
  const glyphs = useMemo(
    () =>
      layout.lines.flatMap((line, lineIndex) => {
        const boxes = line.words.length > 0 ? line.words : [line];
        return boxes.map((box, index) =>
          toGlyph(box, lineIndex, index, layout.width, layout.height),
        );
      }),
    [layout],
  );

  return (
    <div
      ref={ref}
      className={cx("text-layer absolute top-0 left-0 origin-top-left", highlight && "highlight")}
      style={{ width: layout.width, height: layout.height }}
    >
      {glyphs.map((glyph) => (
        <span
          key={glyph.key}
          data-line={glyph.line}
          style={{
            left: glyph.left,
            top: glyph.top,
            fontSize: glyph.fontSize,
            fontFamily: FONT,
            transform: `scaleX(${glyph.scaleX})`,
          }}
        >
          {glyph.text}
        </span>
      ))}
    </div>
  );
});

/** Text of the selected words, with spaces between words and newlines between lines. */
export const selectedText = (container: HTMLElement | null): string => {
  const selection = window.getSelection();
  if (container === null || selection === null || selection.isCollapsed) return "";
  const lines = new Map<string, Array<string>>();
  for (const span of container.querySelectorAll<HTMLSpanElement>("span[data-line]")) {
    if (!selection.containsNode(span, true)) continue;
    const line = span.dataset.line ?? "";
    lines.set(line, [...(lines.get(line) ?? []), span.textContent ?? ""]);
  }
  return [...lines.values()].map((words) => words.join(" ")).join("\n");
};
