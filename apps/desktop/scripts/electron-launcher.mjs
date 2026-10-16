import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);

export const desktopDir = join(dirname(fileURLToPath(import.meta.url)), "..");

export function resolveElectronPath() {
  return require("electron");
}

export function createElectronEnv(extraEnv = {}) {
  const env = { ...process.env, ...extraEnv };
  delete env.ELECTRON_RUN_AS_NODE;
  return env;
}
