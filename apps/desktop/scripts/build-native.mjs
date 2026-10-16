// Compiles the native helper (native/helper.swift) into dist-electron/bin/ss-helper.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, statSync } from "node:fs";
import { join } from "node:path";

import { desktopDir } from "./electron-launcher.mjs";

const source = join(desktopDir, "native", "helper.swift");
const outDir = join(desktopDir, "dist-electron", "bin");
const output = join(outDir, "ss-helper");

if (existsSync(output) && statSync(output).mtimeMs > statSync(source).mtimeMs) {
  process.exit(0);
}

mkdirSync(outDir, { recursive: true });
const result = spawnSync(
  "swiftc",
  [
    "-O",
    "-target",
    `${process.arch === "arm64" ? "arm64" : "x86_64"}-apple-macos13`,
    source,
    "-o",
    output,
  ],
  {
    stdio: "inherit",
  },
);
if (result.error) {
  console.error("swiftc not found — install the Xcode Command Line Tools: xcode-select --install");
  process.exit(1);
}
process.exit(result.status ?? 1);
