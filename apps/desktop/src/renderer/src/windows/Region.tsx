import { useEffect, useState } from "react";

import { run } from "../lib/hooks";
import { invoke } from "../lib/ipc";

interface Point {
  readonly x: number;
  readonly y: number;
}

const MIN_SIZE = 8;

/** Full-display overlay for picking the area to record. */
export function Region() {
  const [start, setStart] = useState<Point | null>(null);
  const [end, setEnd] = useState<Point | null>(null);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") run(invoke("region:cancel"));
      if (event.key === "Enter") run(invoke("region:submit", { rect: null }));
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const rect =
    start && end
      ? {
          x: Math.min(start.x, end.x),
          y: Math.min(start.y, end.y),
          width: Math.abs(end.x - start.x),
          height: Math.abs(end.y - start.y),
        }
      : null;

  return (
    <div
      className="fixed inset-0 cursor-crosshair animate-fade-in"
      onMouseDown={(event) => {
        setStart({ x: event.clientX, y: event.clientY });
        setEnd({ x: event.clientX, y: event.clientY });
      }}
      onMouseMove={(event) => {
        if (start) setEnd({ x: event.clientX, y: event.clientY });
      }}
      onMouseUp={() => {
        if (rect && rect.width >= MIN_SIZE && rect.height >= MIN_SIZE) {
          run(invoke("region:submit", { rect }));
        } else {
          setStart(null);
          setEnd(null);
        }
      }}
    >
      {rect === null ? (
        <div className="absolute inset-0 bg-black/25" />
      ) : (
        <div
          className="absolute rounded-[2px] shadow-[0_0_0_9999px_rgb(0_0_0/0.32)] outline outline-1 outline-white/90"
          style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }}
        >
          <span className="absolute -bottom-7 left-0 rounded-md bg-black/70 px-1.5 py-0.5 font-mono text-[11px] whitespace-nowrap text-white tabular-nums">
            {Math.round(rect.width)} × {Math.round(rect.height)}
          </span>
        </div>
      )}

      {start === null && (
        <div className="pointer-events-none absolute top-[18%] left-1/2 -translate-x-1/2 animate-rise-in rounded-2xl bg-white/90 px-5 py-3 text-center shadow-xl backdrop-blur-xl">
          <div className="flex items-center justify-center gap-2 text-[13px] font-semibold text-ink">
            <span className="size-2 rounded-full bg-red-500 shadow-[0_0_0_3px_rgb(239_68_68/0.2)]" />
            Drag to choose what to record
          </div>
          <div className="mt-1 text-[11.5px] text-muted">
            ↩ whole screen · Esc cancel · click the menu bar timer to stop
          </div>
        </div>
      )}
    </div>
  );
}
