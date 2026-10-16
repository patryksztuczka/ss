import { StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";

import { Editor } from "./windows/Editor";
import { Onboarding } from "./windows/Onboarding";
import { Preview } from "./windows/Preview";
import { Region } from "./windows/Region";
import { Settings } from "./windows/Settings";
import "./index.css";

const container = document.getElementById("root");

if (!container) {
  throw new Error("Root container not found");
}

// Every window loads the same bundle; the hash picks the screen, e.g. "#/editor?id=…".
const [route = "", query = ""] = window.location.hash.replace(/^#\/?/, "").split("?");
const params = new URLSearchParams(query);

const screens: Record<string, () => ReactNode> = {
  editor: () => <Editor id={params.get("id") ?? ""} />,
  onboarding: () => <Onboarding />,
  preview: () => <Preview />,
  region: () => <Region />,
  settings: () => <Settings />,
};

// Floating windows are transparent; normal windows get the app background.
if (route === "preview" || route === "region") {
  document.documentElement.style.background = "transparent";
} else {
  document.documentElement.style.background = "var(--color-canvas)";
}

createRoot(container).render(<StrictMode>{(screens[route] ?? screens.settings!)()}</StrictMode>);
