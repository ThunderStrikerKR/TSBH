#!/usr/bin/env node
/**
 * BANDO HUB visual verification gate.
 * A green build/contract test is NEVER a visual acceptance result.
 * This gate fails closed until a human-reviewed, version-bound screenshot
 * record exists for every changed UI source.
 */
import { readFileSync, existsSync, readdirSync, appendFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { resolve, dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

export const REQUIRED_CHECKS = [
  "noClipping", "footerVisible", "scrollReachable", "inputReadable",
  "alignment", "interaction", "hoverAndFocus",
];

export function isUiSource(filename) {
  const f = filename.replaceAll("\\", "/");
  if (/^src\/.*\.(tsx|css)$/.test(f)) return true;
  return /^egui-app\/src\/(?:main|components|theme|typography|[a-z0-9_]*(?:workspace|manager|view|panel|dialog|modal|ui|search|highlight))\.rs$/.test(f);
}
export function isAuditFile(filename) {
  return /^docs\/ui-visual-verification\/audits\/[a-z0-9][a-z0-9-]*\.json$/.test(filename.replaceAll("\\", "/"));
}
function nonEmpty(s) { return typeof s === "string" && s.trim().length > 0; }
function isSha(s) { return typeof s === "string" && /^[0-9a-f]{40}$/.test(s); }
function safeEvidencePath(s) {
  return typeof s === "string" &&
    /^docs\/ui-visual-verification\/evidence\/[a-zA-Z0-9/_-]+\.png$/.test(s) &&
    !s.split("/").includes("..");
}
function pngHeader(bytes) {
  return bytes.length >= 24 && bytes.subarray(0, 8).equals(
    Buffer.from([137,80,78,71,13,10,26,10])
  ) && bytes.toString("ascii", 12, 16) === "IHDR";
}

/** This validates evidence *metadata*, not pixels or visual quality. */
export function validateVisualAudit(audit, {
  fileExists = () => false,
  fileBytes = () => Buffer.alloc(0),
  blobSha = () => null,
} = {}) {
  const errors = [];
  const bad = (msg) => errors.push(msg);
  if (!audit || typeof audit !== "object" || Array.isArray(audit)) {
    return ["Audit must be an object"];
  }
  if (audit.schemaVersion !== 1) bad("schemaVersion must be 1");
  if (!nonEmpty(audit.id)) bad("id is required");
  if (!nonEmpty(audit.screen)) bad("screen is required");
  if (!Array.isArray(audit.files) || !audit.files.length ||
      audit.files.some((f) => !isUiSource(f)) ||
      new Set(audit.files).size !== audit.files.length) {
    bad("files must be unique UI source paths");
  }
  if (!["PASS", "FAIL", "NOT_VERIFIED"].includes(audit.visualStatus)) {
    bad("visualStatus must be PASS, FAIL or NOT_VERIFIED");
    return errors;
  }
  if (audit.visualStatus === "FAIL") {
    if (!Array.isArray(audit.defects) || !audit.defects.some(
      (d) => nonEmpty(d.id) && nonEmpty(d.description) && d.status === "OPEN"
    )) bad("FAIL requires a documented OPEN defect");
  }
  if (audit.visualStatus === "NOT_VERIFIED" && !nonEmpty(audit.reason)) {
    bad("NOT_VERIFIED requires a reason");
  }
  if (audit.visualStatus !== "PASS") return errors;

  if (!audit.review || !nonEmpty(audit.review.reviewer) ||
      !nonEmpty(audit.review.reviewedAt) ||
      audit.review.platform !== "Windows") {
    bad("PASS requires a named Windows reviewer and review date");
  }
  if (!Array.isArray(audit.defects) ||
      audit.defects.some((d) => d.status !== "CLOSED")) {
    bad("PASS requires a defects array with no unresolved defect");
  }
  for (const f of audit.files || []) {
    const sha = audit.sourceBlobs?.[f];
    if (!isSha(sha) || sha !== blobSha(f)) {
      bad("PASS screenshot must match the current Git blob of " + f);
    }
  }
  const cases = audit.review?.cases;
  if (!Array.isArray(cases) || cases.length < 2) {
    bad("PASS requires at least two distinct Windows viewport cases");
    return errors;
  }
  const widths = new Set();
  for (const [i, item] of cases.entries()) {
    const prefix = "case " + (i + 1) + ": ";
    const width = item?.viewport?.width;
    const height = item?.viewport?.height;
    const scale = item?.viewport?.scalePercent;
    if (!Number.isInteger(width) || !Number.isInteger(height) ||
        width < 800 || height < 600 ||
        !Number.isInteger(scale) || scale < 100 || scale > 250) {
      bad(prefix + "viewport width, height and DPI scale are required");
    } else {
      widths.add(width);
    }
    for (const name of REQUIRED_CHECKS) {
      if (item?.checks?.[name] !== true) bad(prefix + name + " is not PASS");
    }
    const screenshot = item?.screenshot;
    if (!safeEvidencePath(screenshot) || !fileExists(screenshot)) {
      bad(prefix + "sanitized in-repo PNG evidence is required");
    } else {
      let bytes;
      try { bytes = fileBytes(screenshot); } catch { bytes = Buffer.alloc(0); }
      if (!pngHeader(bytes)) bad(prefix + "screenshot is not a valid PNG");
    }
  }
  if (widths.size < 2 || ![...widths].some((w) => w <= 1500) ||
      ![...widths].some((w) => w >= 1920)) {
    bad("PASS needs a 1920px-or-larger and a 1500px-or-smaller viewport");
  }
  return errors;
}

/** Compare only changed UI sources, not the entire historical backlog. */
export function checkVisualChanges(changedPaths, readAudit, env = {}) {
  const changes = [...new Set(changedPaths.map((p) => p.replaceAll("\\", "/")))];
  const ui = changes.filter(isUiSource);
  const records = changes.filter(isAuditFile);
  const errors = [], results = [];
  if (ui.length && !records.length) {
    errors.push("UI source changed without a changed visual audit JSON");
  }
  const covered = new Set();
  for (const path of records) {
    let audit;
    try { audit = readAudit(path); }
    catch (e) { errors.push(path + ": cannot read audit: " + e.message); continue; }
    const problems = validateVisualAudit(audit, env);
    errors.push(...problems.map((e) => path + ": " + e));
    if (audit?.visualStatus !== "PASS") {
      errors.push(path + ": visual status is " +
        String(audit?.visualStatus || "MISSING") + ", not PASS");
    }
    for (const f of audit?.files || []) covered.add(f);
    results.push({ path, status: audit?.visualStatus || "INVALID" });
  }
  for (const f of ui) {
    if (!covered.has(f)) errors.push("Missing updated visual audit coverage: " + f);
  }
  return { pass: errors.length === 0, ui, records: results, errors };
}
function git(...args) {
  return execFileSync("git", args, { encoding: "utf8", stdio: ["ignore","pipe","pipe"] }).trim();
}
function proofEnv() {
  const root = process.cwd();
  return {
    fileExists: (f) => existsSync(join(root, f)),
    fileBytes: (f) => readFileSync(join(root, f)),
    blobSha: (f) => {
      try { return git("hash-object", "--", f); } catch { return null; }
    },
  };
}
function auditFiles() {
  const dir = "docs/ui-visual-verification/audits";
  return readdirSync(dir).filter((f) => f.endsWith(".json")).map((f) => dir + "/" + f);
}
function main() {
  const arg = process.argv[2];
  let paths;
  if (arg === "--base" && process.argv[3]) {
    paths = git("diff", "--name-only", process.argv[3] + "...HEAD").split("\n").filter(Boolean);
  } else if (arg === "--all") {
    paths = auditFiles();
  } else if (arg === "--audit" && process.argv[3]) {
    paths = [process.argv[3]];
  } else {
    throw Error("Usage: node scripts/check-ui-visual-verification.mjs --base <git-sha> | --all | --audit <path>");
  }
  const result = checkVisualChanges(
    paths, (f) => JSON.parse(readFileSync(f, "utf8")), proofEnv()
  );
  const headline = result.pass ? "PASS: evidence gate met" :
    "BLOCKED: no visual approval (build status is independent)";
  const lines = [
    "## UI Visual Verification",
    "**" + headline + "**",
    "Changed UI files: " + result.ui.length + "; audit records: " + result.records.length,
    ...result.records.map((r) => "- " + r.path + ": " + r.status),
    ...result.errors.map((e) => "- BLOCKER: " + e),
    "",
    "A successful static check NEVER proves absence of clipping. Visual PASS requires human-reviewed screenshots.",
  ];
  console.log(lines.join("\n"));
  if (process.env.GITHUB_STEP_SUMMARY) {
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, lines.join("\n") + "\n");
  }
  if (!result.pass) process.exitCode = 1;
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) {
    console.error("UI visual verification failed closed:", error.message);
    process.exitCode = 1;
  }
}
