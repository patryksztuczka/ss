// Regenerates tests/fixtures/compose/ from the Electron app's real compositor:
//   node scripts/fixtures/compose-reference/run.mjs [source.png out-dir]
// With arguments it renders another source into a scratch folder instead.
import { execFileSync } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../..");
const { rolldown } = await import(
  join(root, "node_modules/.pnpm/@voidzero-dev+vite-plus-core@1.1.0_@types+node@26.6.5_jiti@2.7.0_typescript@7.0.2/node_modules/@voidzero-dev/vite-plus-core/dist/rolldown/index.mjs")
);

const base = { padding: 36, inset: 0, radius: 30, shadow: 45, background: "sky", customColor: "#e8e4df", ratio: "auto" };
const styles = [
  base,
  { ...base, background: "desktop", padding: 60, radius: 60, shadow: 80 },
  { ...base, background: "none", padding: 30, shadow: 50 },
  { ...base, background: "custom", customColor: "#da8216", padding: 20, radius: 0, shadow: 0 },
  { ...base, background: "love", padding: 42, radius: 100, shadow: 66, ratio: "16:9" },
  { ...base, background: "rain", inset: 40, radius: 100, shadow: 66, ratio: "1:1" },
];

const work = mkdtempSync(join(tmpdir(), "ss-compose-bundle-"));
const bundle = await rolldown({ input: join(here, "entry.ts"), cwd: join(root, "apps/desktop") });
await bundle.write({ file: join(work, "bundle.js"), format: "iife" });

const electron = join(root, "apps/desktop/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron");
const [sourceArg, outArg] = process.argv.slice(2);
const out = outArg ? resolve(outArg) : join(root, "tests/fixtures/compose");
const source = sourceArg ? resolve(sourceArg) : join(out, "source.png");
// A host app may export this, which would run Electron as plain Node.
const env = { ...process.env };
delete env.ELECTRON_RUN_AS_NODE;
execFileSync(electron, [join(here, "electron-main.cjs")], {
  stdio: "inherit",
  env: {
    ...env,
    SS_BUNDLE: join(work, "bundle.js"),
    SS_SOURCE: source,
    SS_STYLES: JSON.stringify(styles),
    SS_OUT: out,
  },
});
console.log(`wrote ${styles.length} cases to ${out}`);
