import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const root = fileURLToPath(new URL(".", import.meta.url));
const version: string = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")).version;
let buildId = "확인 불가";
try {
  buildId = execFileSync("git", ["rev-parse", "--short=7", "HEAD"], {
    cwd: root,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "ignore"],
  }).trim() || buildId;
} catch {
  // A source export without .git still builds; no fabricated commit ID.
}

/**
 * Vite/Rollup's actual emitted chunk module list is authoritative here.
 * A no-hit substring scan of minified JS is not proof of module exclusion.
 * This guards every output chunk, including lazy developer TEST screens.
 * The historical TypeScript implementations remain available to test oracles
 * but must never ship as executable frontend business rules.
 */
const noLegacyRecognitionOrValidation: Plugin = {
  name: "bando-native-only-recognition-validation",
  generateBundle(_options, bundle) {
    const blocked = new Set<string>();
    let chunks = 0;
    for (const emitted of Object.values(bundle)) {
      if (emitted.type !== "chunk") continue;
      chunks++;
      for (const moduleId of Object.keys(emitted.modules)) {
        const sourcePath = moduleId.replaceAll("\\", "/").split("?")[0];
        if (/\/src\/(?:parser|validation)\.ts$/.test(sourcePath))
          blocked.add(sourcePath.slice(sourcePath.lastIndexOf("/src/") + 1));
      }
    }
    console.log("BANDO_NATIVE_BUNDLE_BOUNDARY", JSON.stringify({
      inspectedChunks: chunks,
      forbiddenModules: ["src/parser.ts", "src/validation.ts"],
      includedForbiddenModules: [...blocked].sort(),
    }));
    if (blocked.size)
      this.error("Rust-owned legacy TS algorithms reached the frontend bundle: " +
        [...blocked].sort().join(", "));
  },
};

export default defineConfig({
  base: "./",
  plugins: [react(), tailwindcss(), noLegacyRecognitionOrValidation],
  define: {
    __BANDO_VERSION__: JSON.stringify(version),
    __BANDO_BUILD_ID__: JSON.stringify(buildId),
  },
});
