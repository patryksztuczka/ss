import { defineConfig } from "vite-plus";

export default defineConfig({
  staged: {
    "*": "vp fmt",
  },
  fmt: {
    ignorePatterns: ["dist", "dist-electron", "node_modules", "pnpm-lock.yaml", "*.tsbuildinfo"],
    sortPackageJson: {},
  },
  lint: {
    ignorePatterns: ["dist", "dist-electron", "node_modules"],
  },
});
