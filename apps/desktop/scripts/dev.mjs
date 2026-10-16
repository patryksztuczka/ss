import { spawn, spawnSync } from "node:child_process";
import { connect } from "node:net";
import { setTimeout as delay } from "node:timers/promises";

import { createElectronEnv, desktopDir, resolveElectronPath } from "./electron-launcher.mjs";

const DEV_SERVER_PORT = 5183;
const DEV_SERVER_URL = `http://localhost:${DEV_SERVER_PORT}`;

function waitForPort(port, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  return new Promise((resolve, reject) => {
    const attempt = () => {
      const socket = connect({ port, host: "localhost" }, () => {
        socket.destroy();
        resolve();
      });
      socket.on("error", () => {
        socket.destroy();
        if (Date.now() > deadline) {
          reject(new Error(`Renderer dev server did not start on port ${port}`));
          return;
        }
        void delay(200).then(attempt);
      });
    };
    attempt();
  });
}

for (const [command, args] of [
  ["vp", ["pack"]],
  ["node", ["scripts/build-native.mjs"]],
]) {
  const result = spawnSync(command, args, { stdio: "inherit", cwd: desktopDir });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

const viteDevServer = spawn("vp", ["dev"], { stdio: "inherit", cwd: desktopDir });

await waitForPort(DEV_SERVER_PORT);

const electron = spawn(resolveElectronPath(), ["dist-electron/main.cjs"], {
  stdio: "inherit",
  cwd: desktopDir,
  env: createElectronEnv({ VITE_DEV_SERVER_URL: DEV_SERVER_URL }),
});

electron.on("exit", (code) => {
  viteDevServer.kill();
  process.exit(code ?? 0);
});

viteDevServer.on("exit", (code) => {
  if (electron.exitCode === null) {
    electron.kill();
  }
  process.exit(code ?? 0);
});
