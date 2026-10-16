import type { BackgroundId } from "../../../shared/settings";

/** A soft radial glow; position and radius are relative to the canvas. */
interface Blob {
  readonly x: number;
  readonly y: number;
  readonly r: number;
  readonly color: string;
}

interface GradientSpec {
  readonly angle: number;
  readonly stops: ReadonlyArray<string>;
  readonly blobs?: ReadonlyArray<Blob>;
}

export interface Background {
  readonly id: BackgroundId;
  readonly label: string;
  readonly spec: GradientSpec | "transparent" | "custom";
}

export const backgrounds: ReadonlyArray<Background> = [
  {
    id: "desktop",
    label: "Desktop",
    spec: {
      angle: 160,
      stops: ["#ffb347", "#ff7a2f", "#e5482c"],
      blobs: [
        { x: 0.25, y: 0.2, r: 0.55, color: "#ffe08a" },
        { x: 0.85, y: 0.85, r: 0.6, color: "#c92f2f" },
        { x: 0.65, y: 0.35, r: 0.35, color: "#ffc06b" },
      ],
    },
  },
  {
    id: "cool",
    label: "Cool",
    spec: {
      angle: 160,
      stops: ["#5aa7f5", "#3d7fe0", "#4c9ae8"],
      blobs: [{ x: 0.15, y: 0.1, r: 0.6, color: "#7fd0ff" }],
    },
  },
  {
    id: "nice",
    label: "Nice",
    spec: {
      angle: 140,
      stops: ["#c44f80", "#d64a74", "#b04a8a"],
      blobs: [{ x: 0.85, y: 0.15, r: 0.5, color: "#e66a8c" }],
    },
  },
  {
    id: "morning",
    label: "Morning",
    spec: {
      angle: 140,
      stops: ["#f5a983", "#ee8f73", "#f4b78d"],
      blobs: [{ x: 0.2, y: 0.85, r: 0.5, color: "#ffc9a3" }],
    },
  },
  {
    id: "bright",
    label: "Bright",
    spec: {
      angle: 135,
      stops: ["#9a72f2", "#6f7ff4", "#b479ef"],
      blobs: [{ x: 0.9, y: 0.1, r: 0.5, color: "#8fb2ff" }],
    },
  },
  {
    id: "love",
    label: "Love",
    spec: {
      angle: 150,
      stops: ["#3b1d8f", "#6a2bd1", "#9a3fe0"],
      blobs: [{ x: 0.85, y: 0.9, r: 0.6, color: "#c54ad6" }],
    },
  },
  {
    id: "rain",
    label: "Rain",
    spec: {
      angle: 135,
      stops: ["#f7a8c8", "#f39ab8"],
      blobs: [
        { x: 0.1, y: 0.15, r: 0.6, color: "#6cb8ff" },
        { x: 0.9, y: 0.2, r: 0.55, color: "#ff6f96" },
        { x: 0.55, y: 0.95, r: 0.6, color: "#ffa36e" },
        { x: 0.15, y: 0.85, r: 0.45, color: "#a08dff" },
      ],
    },
  },
  {
    id: "sky",
    label: "Sky",
    spec: {
      angle: 135,
      stops: ["#b8d3ff", "#c3c5ff"],
      blobs: [
        { x: 0.05, y: 0.05, r: 0.65, color: "#8fe6f8" },
        { x: 0.95, y: 0.1, r: 0.55, color: "#c8b8ff" },
        { x: 0.05, y: 0.95, r: 0.6, color: "#9a7cf2" },
        { x: 0.95, y: 0.95, r: 0.6, color: "#7fcaf6" },
        { x: 0.5, y: 0.5, r: 0.45, color: "#d6e4ff" },
      ],
    },
  },
  { id: "none", label: "None", spec: "transparent" },
  { id: "custom", label: "Custom", spec: "custom" },
];

const withAlpha = (hex: string, alpha: number) => {
  const value = Number.parseInt(hex.slice(1), 16);
  return `rgba(${(value >> 16) & 255}, ${(value >> 8) & 255}, ${value & 255}, ${alpha})`;
};

/** CSS for swatches, generated from the same spec the canvas uses so they always match. */
export const backgroundCss = (background: Background, customColor: string): string => {
  if (background.spec === "transparent") {
    return "repeating-conic-gradient(#e7e4e0 0% 25%, #fbfaf8 0% 50%) 50% / 12px 12px";
  }
  if (background.spec === "custom") return customColor;
  const { angle, stops, blobs = [] } = background.spec;
  const glows = blobs.map(
    (blob) =>
      `radial-gradient(circle at ${blob.x * 100}% ${blob.y * 100}%, ${blob.color} 0%, ${withAlpha(blob.color, 0)} ${blob.r * 100}%)`,
  );
  // CSS paints the first layer on top; the canvas paints blobs in order, so reverse them.
  return [...glows.reverse(), `linear-gradient(${angle}deg, ${stops.join(", ")})`].join(", ");
};

export const paintBackground = (
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  background: Background,
  customColor: string,
) => {
  ctx.clearRect(0, 0, width, height);
  if (background.spec === "transparent") return;
  if (background.spec === "custom") {
    ctx.fillStyle = customColor;
    ctx.fillRect(0, 0, width, height);
    return;
  }
  const { angle, stops, blobs = [] } = background.spec;
  // CSS angle semantics: 0deg points up, increasing clockwise.
  const radians = ((angle - 90) * Math.PI) / 180;
  const half = (Math.abs(width * Math.cos(radians)) + Math.abs(height * Math.sin(radians))) / 2;
  const cx = width / 2;
  const cy = height / 2;
  const linear = ctx.createLinearGradient(
    cx - Math.cos(radians) * half,
    cy - Math.sin(radians) * half,
    cx + Math.cos(radians) * half,
    cy + Math.sin(radians) * half,
  );
  stops.forEach((color, index) =>
    linear.addColorStop(index / Math.max(1, stops.length - 1), color),
  );
  ctx.fillStyle = linear;
  ctx.fillRect(0, 0, width, height);

  // CSS `circle` percentages resolve against the diagonal / √2; mirror that so swatches match.
  const reference = Math.hypot(width, height) / Math.SQRT2;
  for (const blob of blobs) {
    const radius = blob.r * reference;
    const radial = ctx.createRadialGradient(
      blob.x * width,
      blob.y * height,
      0,
      blob.x * width,
      blob.y * height,
      radius,
    );
    radial.addColorStop(0, blob.color);
    radial.addColorStop(1, withAlpha(blob.color, 0));
    ctx.fillStyle = radial;
    ctx.fillRect(0, 0, width, height);
  }
};

export const backgroundById = (id: BackgroundId): Background =>
  backgrounds.find((background) => background.id === id) ?? backgrounds[0]!;
