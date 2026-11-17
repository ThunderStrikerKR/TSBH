import { execFileSync } from "node:child_process";
import { appendFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

// Fail closed to the full GUI suite for unknown or mixed changes.
const WINDOW_FILES = new Set([
  "scripts/verify-dev-window-placement-win.ps1",
  "tests/tauri-window-placement.test.mjs",
  "tests/dev-window-placement-real-restart.test.mjs",
  "tests/ci-windows-smoke-routing.test.mjs",
  "tests/ci-dev-domain-gui-contract.test.mjs",
]);
const TEST_FILES = new Set([
  "tests/tauri-window-placement.test.mjs",
  "tests/dev-window-placement-real-restart.test.mjs",
  "tests/ci-windows-smoke-routing.test.mjs",
  "tests/ci-dev-domain-gui-contract.test.mjs",
]);
const FOCUSED_PROBES = new Map([
  ["scripts/migration32-real-factory-notifications.mjs","factory"],
  ["scripts/ci-domains/orders.mjs","orders"],
  ["scripts/ci-domains/pricing.mjs","pricing"],
  ["scripts/ci-domains/shipping.mjs","shipping"],
  ["scripts/ci-domains/clients.mjs","clients"],
  ["scripts/ci-domains/factory.mjs","factory"],
]);
export const DOMAIN_SCOPES=["orders","pricing","shipping","clients","factory"];
const LIB = "src-tauri/src/lib.rs";
const BLOCKS = [
  ["// The window-state plugin can restore", "struct Database {"],
  ["// Restore only the BANDO HUB main window.", ".setup(move |app| {"],
];

export function stripWindowSections(input) {
  if (typeof input !== "string") return null;
  const source = input.replace(/\r\n/g, "\n");
  const found = [];
  for (const [start, end] of BLOCKS) {
    const begin = source.indexOf(start);
    const finish = source.indexOf(end, begin + start.length);
    if (begin < 0 || finish < 0 ||
        source.indexOf(start, begin + start.length) !== -1 ||
        source.indexOf(end, finish + end.length) !== -1) return null;
    found.push({ begin, finish });
  }
  if (found[0].finish >= found[1].begin) return null;
  return source.slice(0, found[0].begin) + "/* WINDOW_NATIVE */\n" +
    source.slice(found[0].finish, found[1].begin) + "/* WINDOW_PLUGIN */\n" +
    source.slice(found[1].finish);
}

export function selectWindowsScope(paths, previousLib, currentLib) {
  if (!Array.isArray(paths) || paths.length === 0) return "full";
  let testOnly = true;
  let focused = null;
  for (const path of paths) {
    if (FOCUSED_PROBES.has(path)) {
      if (focused && focused !== FOCUSED_PROBES.get(path)) return "full";
      focused = FOCUSED_PROBES.get(path);
      testOnly = false;
      continue;
    }
    if (path === LIB) {
      testOnly = false;
      const before = stripWindowSections(previousLib);
      const after = stripWindowSections(currentLib);
      if (before === null || after === null || before !== after) return "full";
    } else if (!WINDOW_FILES.has(path)) {
      return "full";
    } else if (!TEST_FILES.has(path)) {
      testOnly = false;
    }
  }
  return focused ? (paths.every(p=>FOCUSED_PROBES.has(p) || TEST_FILES.has(p)) ? focused : "full")
    : testOnly ? "fast" : "window";
}

function git(...args) {
  return execFileSync("git", args, { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 }).trimEnd();
}

function main() {
  const override = process.env.CI_OVERRIDE_SCOPE || "";
  let scope = "full";
  let paths = [];
  if (override) {
    if (!["full", "window", ...DOMAIN_SCOPES].includes(override)) throw new Error("Invalid scope override");
    scope = override;
  } else {
    const head = process.env.CI_HEAD_SHA || "HEAD";
    let base = process.env.CI_BASE_SHA || "";
    if (!base || /^0+$/.test(base)) {
      try { base = git("rev-parse", head + "^"); } catch { base = ""; }
    }
    if (base) {
      try {
        paths = git("diff", "--name-only", "--no-renames", base, head, "--")
          .split("\n").filter(Boolean);
        const before = paths.includes(LIB) ? git("show", base + ":" + LIB) : null;
        const after = paths.includes(LIB) ? git("show", head + ":" + LIB) : null;
        scope = selectWindowsScope(paths, before, after);
      } catch (error) {
        console.warn("Diff unavailable; choosing full Windows GUI:", error.message);
      }
    }
  }
  console.log("BANDO_WINDOWS_SMOKE_PLAN " + JSON.stringify({ scope, paths }));
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, "scope=" + scope + "\n");
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
