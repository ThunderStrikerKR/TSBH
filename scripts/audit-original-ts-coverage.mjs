// Read-only inventory: original pre-Rust TypeScript vs current source and test oracles.
// This is coverage metadata, NOT a behavioral-parity assertion.
// The historical first-commit source is NOT an authoritative modern feature
// specification. Never auto-revert improvements on the basis of this report.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";

const baseline = "30a19cd2da7533dd2ed36b807593309bde70a7eb";
const run = (...args) => execFileSync("git", args, {
  encoding: "utf8", maxBuffer: 20 * 1024 * 1024,
});
function tree(ref) {
  const all = run("ls-tree", "-r", "-z", ref, "--",
    "src", "src-tauri/src/business_core", "tests/rust-migration");
  const entries = new Map();
  for (const row of all.split("\0")) {
    if (!row) continue;
    const tab = row.indexOf("\t");
    if (tab < 0) continue;
    const fields = row.slice(0, tab).split(" ");
    entries.set(row.slice(tab + 1), fields[2]);
  }
  return entries;
}
const old = tree(baseline), current = tree("HEAD");
const isTS = p => p.startsWith("src/") && /\.(ts|tsx|mts)$/.test(p);
const isRust = p => p.startsWith("src-tauri/src/business_core/") && p.endsWith(".rs");
const isTest = p => p.startsWith("tests/rust-migration/") && /\.(mjs|mts|ts)$/.test(p);
const originalFiles = [...old.keys()].filter(isTS).sort();
const currentFiles = [...current.keys()].filter(isTS).sort();
const changedFiles = currentFiles.filter(p => old.get(p) !== current.get(p));
const removedFiles = originalFiles.filter(p => !current.has(p));
const rustFiles = [...current.keys()].filter(isRust).sort();
const testFiles = [...current.keys()].filter(isTest).sort();
const refs = [];
for (const test of testFiles) {
  if (!existsSync(test)) continue;
  const text = readFileSync(test, "utf8");
  // Include TS imports via tsImport, ES imports and new URL.
  const matcher = /(?:tsImport\s*\(\s*|from\s*|import\s*|new URL\s*\(\s*)["']([^"']+)["']/g;
  const linked = new Set();
  for (const match of text.matchAll(matcher)) {
    if (!match[1].startsWith(".")) continue;
    const path = relative(process.cwd(), resolve(dirname(test), match[1])).replaceAll("\\", "/");
    if (isTS(path)) linked.add(path);
  }
  const sources = [...linked].sort().map(path => ({
    path, originalBlob: old.get(path) ?? null, currentBlob: current.get(path) ?? null,
    classification: !old.has(path) ? "ABSENT_FROM_BASELINE" :
      old.get(path) === current.get(path) ? "BYTE_IDENTICAL" : "CHANGED_SINCE_BASELINE",
  }));
  refs.push({
    test, sources,
    unsealed: sources.some(s => s.classification !== "BYTE_IDENTICAL"),
    classification: sources.length ? "DIRECT_TS_IMPORT" : "NO_DIRECT_TS_IMPORT",
  });
}
const candidates = [];
for (const path of currentFiles.filter(p => p.endsWith(".tsx"))) {
  const content = readFileSync(path, "utf8").split("\n");
  for (let i = 0; i < content.length; i++) {
    const context = content.slice(Math.max(0, i - 2), i + 1).join(" ");
    if (/확인 필요|확인 중|계산 중|조회 중/.test(content[i]) &&
        /ready|loading|View|native|preview|result/.test(context)) {
      candidates.push({path, line: i + 1, text: content[i].trim().slice(0, 190)});
    }
  }
}
const summary = {
  baseline, head: run("rev-parse", "HEAD").trim(),
  originalTS: originalFiles.length, currentTS: currentFiles.length,
  changedOrNewTS: changedFiles.length, removedTS: removedFiles.length,
  rustBusinessModules: rustFiles.length, testAndHelperFiles: testFiles.length,
  directMutableTestOracles: refs.filter(r => r.unsealed).length,
  noDirectTSImports: refs.filter(r => r.classification === "NO_DIRECT_TS_IMPORT").length,
  loadingStateCandidates: candidates.length,
  verdict: "HISTORICAL_DIFF_ONLY_NOT_A_CURRENT_FEATURE_CONTRACT",
  policy: "DIFFERENCE_DOES_NOT_AUTHORIZE_REVERT;CHECK_USER_APPROVED_IMPROVEMENTS",
};
const report = {summary, removedFiles, changedFiles, rustFiles, tests: refs, candidates};
mkdirSync("migration-audit", {recursive: true});
writeFileSync("migration-audit/original-ts-coverage.json",
  JSON.stringify(report, null, 2) + "\n", "utf8");
const md = [
  "# Fixed-original TypeScript source-coverage inventory", "",
  "This is an automated historical inventory, NOT the current approved product specification.", 
  "Changes since 2026-11-06 are NOT automatically bugs or deletion candidates.",
  "Preserve subsequent approved improvements; compare intent and workflow before editing.",
  "Frozen pre-migration baseline: " + baseline,
  "Current commit: " + summary.head, "",
  ...Object.entries(summary).map(([key,value]) => "- " + key + ": " + value),
  "", "## Every parity test's direct TypeScript oracle", "",
  "| Test/helper | Direct imported source and historical status |",
  "| --- | --- |",
  ...refs.map(row => "| " + row.test + " | " +
    (row.sources.map(s => s.path + " [" + s.classification + "]").join("; ") || "NO_DIRECT_TS_IMPORT") + " |"),
  "", "## Suspected loading/confirmation UI coupling", "",
  "| Source | Line | Text |", "| --- | ---: | --- |",
  ...candidates.map(c => "| " + c.path + " | " + c.line + " | " +
    c.text.replaceAll("|", "\\|") + " |"),
  "", "Interpretation:",
  "- BYTE_IDENTICAL is source-byte equality, not verified runtime equivalence.",
  "- CHANGED_SINCE_BASELINE means that live TS import cannot by itself prove old TS parity.",
  "- ABSENT_FROM_BASELINE means it cannot be a test oracle from this old snapshot.",
  "- NO_DIRECT_TS_IMPORT may be a helper or an indirect import; investigate manually.",
  "- Every domain still needs frozen executable differentials and real GUI workflow validation.",
  ""
];
writeFileSync("migration-audit/original-ts-coverage.md", md.join("\n"), "utf8");
console.log("FIXED_TS_SOURCE_COVERAGE " + JSON.stringify(summary));
console.log("MUTABLE_ORACLE_TESTS " +
  JSON.stringify(refs.filter(r => r.unsealed).map(r => r.test)));
