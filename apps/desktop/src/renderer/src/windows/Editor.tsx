import { Effect } from "effect";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import type { AspectRatio, EditorStyle } from "../../../shared/settings";
import { TextLayer, selectedText } from "../components/TextLayer";
import { Button, Kbd, Section, Segmented, Slider, cx, useToast } from "../components/ui";
import { backgroundCss, backgrounds } from "../lib/backgrounds";
import {
  computeLayout,
  detectEdgeColor,
  exportPng,
  loadBitmap,
  renderComposition,
} from "../lib/compose";
import { run, useEffectState } from "../lib/hooks";
import { invoke } from "../lib/ipc";

const RATIOS: ReadonlyArray<{ value: AspectRatio; label: string }> = [
  { value: "auto", label: "Auto" },
  { value: "16:9", label: "16:9" },
  { value: "4:3", label: "4:3" },
  { value: "3:2", label: "3:2" },
  { value: "1:1", label: "1:1" },
];

const STAGE_GUTTER = 56;

const loadEditor = (id: string) =>
  Effect.gen(function* () {
    const [capture, settings] = yield* Effect.all(
      [invoke("capture:load", { id }), invoke("settings:get")],
      {
        concurrency: "unbounded",
      },
    );
    const bitmap = yield* loadBitmap(capture.png);
    return { capture, bitmap, insetColor: detectEdgeColor(bitmap), style: settings.editor };
  });

export function Editor({ id }: { id: string }) {
  const state = useEffectState(() => loadEditor(id), [id]);

  if (state._tag === "Failure") {
    return (
      <div className="grid h-full place-items-center bg-canvas text-[13px] text-ink-soft">
        <div className="drag fixed inset-x-0 top-0 h-12" />
        This capture is no longer available.
      </div>
    );
  }
  if (state._tag === "Loading") {
    return (
      <div className="flex h-full bg-canvas">
        <div className="drag fixed inset-x-0 top-0 h-12" />
        <div className="m-14 flex-1 animate-shimmer rounded-2xl bg-[linear-gradient(90deg,#ece9e5_0%,#f4f2ef_50%,#ece9e5_100%)] bg-[length:200%_100%]" />
        <div className="w-[300px] border-l border-line bg-panel" />
      </div>
    );
  }
  return <EditorView id={id} {...state.value} />;
}

function EditorView({
  id,
  bitmap,
  insetColor,
  style: initialStyle,
}: {
  id: string;
  bitmap: ImageBitmap;
  insetColor: string;
  style: EditorStyle;
}) {
  const [style, setStyle] = useState(initialStyle);
  const [highlight, setHighlight] = useState(false);
  const [stage, setStage] = useState({ width: 0, height: 0 });
  const stageRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const textRef = useRef<HTMLDivElement>(null);
  const colorInputRef = useRef<HTMLInputElement>(null);
  const toast = useToast();
  const ocr = useEffectState(() => invoke("capture:ocr", { id }), [id]);

  const layout = useMemo(() => computeLayout(style, bitmap.width, bitmap.height), [style, bitmap]);
  const displayScale = Math.max(
    0.01,
    Math.min(
      (stage.width - STAGE_GUTTER * 2) / layout.width,
      (stage.height - STAGE_GUTTER * 2) / layout.height,
      1,
    ),
  );

  const update = useCallback(
    (patch: Partial<EditorStyle>) => setStyle((current) => ({ ...current, ...patch })),
    [],
  );

  // Remember the look for next time (debounced).
  useEffect(() => {
    const timer = window.setTimeout(() => run(invoke("settings:update", { editor: style })), 400);
    return () => window.clearTimeout(timer);
  }, [style]);

  useLayoutEffect(() => {
    const element = stageRef.current;
    if (element === null) return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setStage({ width: entry.contentRect.width, height: entry.contentRect.height });
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const ctx = canvasRef.current?.getContext("2d");
    if (!ctx || stage.width === 0) return;
    const frame = requestAnimationFrame(() =>
      renderComposition(
        ctx,
        bitmap,
        style,
        layout,
        insetColor,
        Math.min(1, displayScale * devicePixelRatio),
      ),
    );
    return () => cancelAnimationFrame(frame);
  }, [bitmap, style, layout, insetColor, displayScale, stage.width]);

  const copyImage = useCallback(
    (thenClose = false) =>
      run(
        exportPng(bitmap, style, insetColor).pipe(
          Effect.flatMap((png) => invoke("clipboard:write-image", { png })),
          Effect.tap(() =>
            Effect.sync(() =>
              thenClose ? void run(invoke("window:close")) : toast.show("Image copied"),
            ),
          ),
          Effect.tapError(() => Effect.sync(() => toast.show("Could not copy the image"))),
        ),
      ),
    [bitmap, style, insetColor, toast],
  );

  const save = useCallback(
    (mode: "quick" | "dialog") =>
      run(
        exportPng(bitmap, style, insetColor).pipe(
          Effect.flatMap((png) => invoke("image:save", { png, mode })),
          Effect.tap((path) =>
            Effect.sync(() => {
              if (path !== null) toast.show(`Saved to ${path.replace(/^\/Users\/[^/]+/, "~")}`);
            }),
          ),
          Effect.tapError(() => Effect.sync(() => toast.show("Could not save the image"))),
        ),
      ),
    [bitmap, style, insetColor, toast],
  );

  const copyText = useCallback(
    (text: string, message: string) =>
      run(
        invoke("clipboard:write-text", { text }).pipe(
          Effect.tap(() => Effect.sync(() => toast.show(message))),
        ),
      ),
    [toast],
  );

  const copyAllText = useCallback(() => {
    if (ocr._tag !== "Success" || ocr.value.lines.length === 0) return toast.show("No text found");
    copyText(ocr.value.lines.map((line) => line.text).join("\n"), "All text copied");
  }, [ocr, copyText, toast]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.target instanceof HTMLInputElement && event.target.type !== "range") return;
      const key = event.key.toLowerCase();
      if (event.metaKey) {
        if (key === "c" && event.shiftKey) return (event.preventDefault(), copyAllText());
        if (key === "c") {
          event.preventDefault();
          const text = selectedText(textRef.current);
          return text !== "" ? copyText(text, "Text copied") : copyImage();
        }
        if (key === "s") return (event.preventDefault(), save(event.shiftKey ? "dialog" : "quick"));
        if (key === "w") return (event.preventDefault(), run(invoke("window:close")));
        return;
      }
      if (event.altKey || event.ctrlKey) return;
      if (key === "escape") {
        const selection = window.getSelection();
        if (selection !== null && !selection.isCollapsed) return selection.removeAllRanges();
        return run(invoke("window:close"));
      }
      if (key === "h") return setHighlight((value) => !value);
      if (key === "r") {
        const index = RATIOS.findIndex((ratio) => ratio.value === style.ratio);
        return update({ ratio: RATIOS[(index + 1) % RATIOS.length]!.value });
      }
      if (key === "[" || key === "]") {
        const delta = key === "[" ? -4 : 4;
        return update({ padding: Math.min(100, Math.max(0, style.padding + delta)) });
      }
      const digit = "1234567890".indexOf(key);
      if (digit >= 0 && digit < backgrounds.length) update({ background: backgrounds[digit]!.id });
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [copyAllText, copyImage, copyText, save, style.padding, style.ratio, update]);

  const ocrStatus =
    ocr._tag === "Loading"
      ? "Recognizing text…"
      : ocr._tag === "Failure"
        ? "Text recognition unavailable"
        : ocr.value.lines.length === 0
          ? "No text found"
          : `${ocr.value.lines.length} line${ocr.value.lines.length === 1 ? "" : "s"} · select it right on the image`;

  return (
    <div className="flex h-full bg-canvas">
      <main className="relative flex min-w-0 flex-1 flex-col">
        <header className="drag flex h-12 shrink-0 items-center justify-center">
          <span className="text-[12px] font-medium text-muted tabular-nums">
            {bitmap.width} × {bitmap.height}
          </span>
        </header>

        <div
          ref={stageRef}
          className="relative min-h-0 flex-1"
          onDoubleClick={(event) => {
            if (event.target === event.currentTarget) copyImage(true);
          }}
        >
          <div
            className="absolute top-1/2 left-1/2 -translate-1/2 animate-rise-in"
            style={{ width: layout.width * displayScale, height: layout.height * displayScale }}
          >
            <canvas
              ref={canvasRef}
              className={cx(
                "block size-full",
                style.background === "none" &&
                  "bg-[repeating-conic-gradient(#e9e6e2_0%_25%,#f4f2ef_0%_50%)] bg-[length:16px_16px]",
              )}
            />
            {ocr._tag === "Success" && (
              <div
                className="absolute"
                style={{ left: layout.image.x * displayScale, top: layout.image.y * displayScale }}
              >
                <div style={{ transform: `scale(${displayScale})`, transformOrigin: "0 0" }}>
                  <TextLayer ref={textRef} layout={ocr.value} highlight={highlight} />
                </div>
              </div>
            )}
          </div>
        </div>

        <footer className="flex h-16 shrink-0 items-center gap-2 px-5">
          <Button variant="primary" shortcut="⌘C" onClick={() => copyImage()}>
            Copy
          </Button>
          <Button shortcut="⌘S" onClick={() => save("quick")}>
            Save
          </Button>
          <Button shortcut="⇧⌘S" onClick={() => save("dialog")}>
            Save As…
          </Button>
          <span className="ml-auto text-[12px] text-muted">
            Double-click the background to copy &amp; close ·{" "}
            {Math.round(displayScale * devicePixelRatio * 100)}%
          </span>
        </footer>
      </main>

      <aside className="flex w-[300px] shrink-0 flex-col border-l border-line bg-panel">
        <div className="drag h-12 shrink-0" />
        <div className="flex-1 space-y-7 overflow-y-auto px-5 pb-6">
          <Section title="Spacing">
            <Slider
              label="Padding"
              value={style.padding}
              onChange={(padding) => update({ padding })}
            />
            <Slider
              label="Inset"
              value={style.inset}
              onChange={(inset) => update({ inset })}
              hint={
                <span className="flex items-center gap-1.5 text-[11px] text-muted">
                  edge color
                  <span
                    className="size-3 rounded-full shadow-[inset_0_0_0_0.5px_rgb(0_0_0/0.2)]"
                    style={{ background: insetColor }}
                  />
                </span>
              }
            />
          </Section>

          <Section title="Card">
            <div className="grid grid-cols-2 gap-4">
              <Slider
                label="Radius"
                value={style.radius}
                onChange={(radius) => update({ radius })}
              />
              <Slider
                label="Shadow"
                value={style.shadow}
                onChange={(shadow) => update({ shadow })}
              />
            </div>
          </Section>

          <Section title="Background" aside={<Kbd>1 – 0</Kbd>}>
            <div className="grid grid-cols-5 gap-x-2 gap-y-3">
              {backgrounds.map((background) => {
                const selected = style.background === background.id;
                return (
                  <button
                    type="button"
                    key={background.id}
                    title={background.label}
                    onClick={() => {
                      if (background.id === "custom" && selected) colorInputRef.current?.click();
                      update({ background: background.id });
                    }}
                    className="group flex flex-col items-center gap-1.5"
                  >
                    <span
                      className={cx(
                        "relative block aspect-square w-full rounded-[10px] transition-all duration-300 ease-spring",
                        "shadow-[inset_0_0_0_0.5px_rgb(0_0_0/0.12)] group-hover:scale-[1.06] group-active:scale-95",
                        selected && "ring-2 ring-accent ring-offset-2 ring-offset-panel",
                      )}
                      style={{ background: backgroundCss(background, style.customColor) }}
                    >
                      {background.id === "custom" && (
                        <span className="absolute inset-0 grid place-items-center text-[15px] text-white mix-blend-difference">
                          +
                        </span>
                      )}
                    </span>
                    <span
                      className={cx(
                        "text-[10.5px]",
                        selected ? "font-medium text-ink" : "text-muted",
                      )}
                    >
                      {background.label}
                    </span>
                  </button>
                );
              })}
            </div>
            <input
              ref={colorInputRef}
              type="color"
              className="sr-only"
              value={style.customColor}
              onChange={(event) =>
                update({ background: "custom", customColor: event.currentTarget.value })
              }
            />
          </Section>

          <Section title="Size" aside={<Kbd>R</Kbd>}>
            <Segmented
              options={RATIOS}
              value={style.ratio}
              onChange={(ratio) => update({ ratio })}
            />
          </Section>

          <Section title="Text">
            <p
              className={cx(
                "text-[12px] leading-relaxed text-ink-soft",
                ocr._tag === "Loading" &&
                  "animate-shimmer bg-[linear-gradient(90deg,#9a948d_0%,#d6d2cd_50%,#9a948d_100%)] bg-[length:200%_100%] bg-clip-text text-transparent",
              )}
            >
              {ocrStatus}
            </p>
            <div className="flex gap-2">
              <Button
                className="flex-1 justify-center"
                shortcut="⇧⌘C"
                disabled={ocr._tag !== "Success" || ocr.value.lines.length === 0}
                onClick={copyAllText}
              >
                Copy all
              </Button>
              <Button
                className={cx("justify-center", highlight && "bg-accent-soft! text-accent")}
                shortcut="H"
                disabled={ocr._tag !== "Success" || ocr.value.lines.length === 0}
                onClick={() => setHighlight((value) => !value)}
              >
                Show
              </Button>
            </div>
          </Section>
        </div>
      </aside>
      {toast.node}
    </div>
  );
}
