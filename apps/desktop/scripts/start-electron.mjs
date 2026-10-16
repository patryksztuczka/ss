import { spawn } from "node:child_process";

import { createElectronEnv, desktopDir, resolveElectronPath } from "./electron-launcher.mjs";

const child = spawn(resolveElectronPath(), ["dist-electron/main.cjs"], {
  stdio: "inherit",
  cwd: desktopDir,
  env: createElectronEnv(),
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 0);
});
