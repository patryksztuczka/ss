import { Effect } from "effect";
import { useCallback, useEffect, useRef, useState } from "react";

import type { PreviewPayload } from "../../../shared/ipc";
import { cx } from "../components/ui";
import { run, useEvent } from "../lib/hooks";
import { invoke } from "../lib/ipc";

const VISIBLE_FOR_MS = 6000;

type Phase = "in" | "out";

export function Preview() {
  const [payload, setPayload] = useState<PreviewPayload | null>(null);
  const [phase, setPhase] = useState<Phase>("in");
  const [note, setNote] = useState("Copied");
  const [hovered, setHovered] = useState(false);
  const generation = useRef(0);

  useEvent("preview:show", (next) => {
    generation.current += 1;
    setPayload(next);
    setPhase("in");
    setNote(next.kind === "video" ? "Recording copied" : "Copied");
  });

  const dismiss = useCallback(() => setPhase("out"), []);

  // Auto-hide unless the pointer is resting on it.
  useEffect(() => {
    if (payload === null || hovered || phase === "out") return;
    const timer = window.setTimeout(dismiss, VISIBLE_FOR_MS);
    return () => window.clearTimeout(timer);
  }, [payload, hovered, phase, dismiss]);

  if (payload === null) return null;
  const isImage = payload.kind === "image";

  const act = (effect: Effect.Effect<unknown, unknown>) =>
    run(effect.pipe(Effect.andThen(Effect.sync(dismiss))));

  return (
    <div className="h-full p-[18px]">
      <div
        key={generation.current}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        onAnimationEnd={(event) => {
          if (event.target === event.currentTarget && phase === "out")
            run(invoke("preview:dismiss"));
        }}
        className={cx(
          "group relative size-full overflow-hidden rounded-[14px] bg-white",
          "shadow-[0_0_0_0.5px_rgb(0_0_0/0.14),0_10px_30px_-6px_rgb(20_20_40/0.35),0_2px_6px_rgb(0_0_0/0.08)]",
          phase === "in" ? "animate-slide-in" : "animate-slide-out",
        )}
      >
        <img
          src={payload.thumbnail}
          alt=""
          draggable
          onDragStart={(event) => {
            event.preventDefault();
            run(invoke("preview:drag", { id: payload.id }));
          }}
          onClick={() =>
            act(
              isImage
                ? invoke("preview:edit", { id: payload.id })
                : invoke("preview:reveal", { id: payload.id }),
            )
          }
          className="size-full object-cover transition-transform duration-500 ease-out-soft group-hover:scale-[1.02]"
        />

        {/* Status chip */}
        <div className="pointer-events-none absolute top-2 left-2 flex items-center gap-1 rounded-full bg-black/55 px-2 py-0.5 text-[10.5px] font-medium text-white backdrop-blur-md animate-fade-in">
          <svg
            viewBox="0 0 12 12"
            className="size-2.5"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.8"
          >
            <path d="M2.5 6.3 5 8.6l4.5-5.2" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          {note}
        </div>

        {!isImage && (
          <div className="pointer-events-none absolute inset-0 grid place-items-center">
            <span className="grid size-10 place-items-center rounded-full bg-black/45 text-white backdrop-blur-md">
              <svg viewBox="0 0 16 16" className="ml-0.5 size-4" fill="currentColor">
                <path d="M4 2.8v10.4c0 .6.6.9 1.1.6l8.1-5.2a.7.7 0 0 0 0-1.2L5.1 2.2C4.6 1.9 4 2.2 4 2.8Z" />
              </svg>
            </span>
          </div>
        )}

        {/* Hover actions */}
        <button
          type="button"
          aria-label="Dismiss"
          onClick={dismiss}
          className="absolute top-2 right-2 grid size-6 place-items-center rounded-full bg-white/90 text-ink-soft opacity-0 shadow transition-all duration-200 group-hover:opacity-100 hover:text-ink"
        >
          <svg viewBox="0 0 12 12" className="size-2.5" stroke="currentColor" strokeWidth="1.8">
            <path d="m3 3 6 6M9 3 3 9" strokeLinecap="round" />
          </svg>
        </button>
        <div className="absolute inset-x-2 bottom-2 flex translate-y-2 gap-1.5 opacity-0 transition-all duration-300 ease-out-soft group-hover:translate-y-0 group-hover:opacity-100">
          {isImage ? (
            <>
              <PreviewAction onClick={() => act(invoke("preview:edit", { id: payload.id }))}>
                Edit
              </PreviewAction>
              <PreviewAction
                onClick={() =>
                  run(
                    invoke("preview:copy-text", { id: payload.id }).pipe(
                      Effect.tap((lines) =>
                        Effect.sync(() => setNote(lines > 0 ? "Text copied" : "No text found")),
                      ),
                    ),
                  )
                }
              >
                Copy text
              </PreviewAction>
            </>
          ) : (
            <PreviewAction onClick={() => act(invoke("preview:reveal", { id: payload.id }))}>
              Show in Finder
            </PreviewAction>
          )}
        </div>
      </div>
    </div>
  );
}

function PreviewAction({ children, onClick }: { children: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="h-7 flex-1 rounded-lg bg-white/92 text-[11.5px] font-medium text-ink shadow-[0_0_0_0.5px_rgb(0_0_0/0.1),0_2px_6px_rgb(0_0_0/0.12)] backdrop-blur transition-transform active:scale-95"
    >
      {children}
    </button>
  );
}
