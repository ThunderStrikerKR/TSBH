import test from "node:test";
import assert from "node:assert/strict";
import {
  REQUIRED_CHECKS, isUiSource, isRustTestOnlyDiff, validateVisualAudit, checkVisualChanges,
} from "../scripts/check-ui-visual-verification.mjs";

const source = "egui-app/src/manual_workspace.rs";
const auditPath = "docs/ui-visual-verification/audits/example.json";
const pngPath = "docs/ui-visual-verification/evidence/redacted.png";
const blob = "a".repeat(40);
const png = Buffer.alloc(24);
Buffer.from([137,80,78,71,13,10,26,10]).copy(png,0);
png.write("IHDR",12,"ascii");

function passAudit() {
  const checks=Object.fromEntries(REQUIRED_CHECKS.map((key)=>[key,true]));
  const viewport=(width,height)=>({
    viewport:{width,height,scalePercent:100},
    checks:{...checks},screenshot:pngPath,
  });
  return {
    schemaVersion:1,id:"example",screen:"수동 분류",
    files:[source],visualStatus:"PASS",
    sourceBlobs:{[source]:blob},defects:[],
    review:{
      reviewer:"Windows QA",reviewedAt:"2026-11-24",platform:"Windows",
      cases:[viewport(1920,1080),viewport(1366,768)],
    },
  };
}
const proof={
  fileExists:(f)=>f===pngPath,
  fileBytes:()=>png,
  blobSha:()=>blob,
};
test("UI source recognition does not misclassify backend-only files",()=>{
  assert.equal(isUiSource(source),true);
  assert.equal(isUiSource("egui-app/src/theme.rs"),true);
  assert.equal(isUiSource("egui-app/src/settings_help.rs"),true);
  assert.equal(isUiSource("egui-app/src/cell_navigation.rs"),true);
  assert.equal(isUiSource("src/orders/ManualClassification.tsx"),true);
  assert.equal(isUiSource("src/styles/ui.css"),true);
  assert.equal(isUiSource("egui-app/src/gateway.rs"),false);
  assert.equal(isUiSource("crates/bando-data-access/src/lib.rs"),false);
});
test("a changed UI source without a changed evidence manifest blocks the gate",()=>{
  const result=checkVisualChanges([source],()=>{throw Error("unreachable");},proof);
  assert.equal(result.pass,false);
  assert.match(result.errors.join(" "),/without a changed visual audit/);
});
test("a changed manifest marked failed must not be reported as UI PASS",()=>{
  const fail={
    ...passAudit(),visualStatus:"FAIL",review:undefined,
    defects:[{id:"UI-84-CLIP",description:"운임 선택 버튼 하단 잘림",status:"OPEN"}],
  };
  assert.deepEqual(validateVisualAudit(fail,proof),[]);
  const result=checkVisualChanges([source,auditPath],()=>fail,proof);
  assert.equal(result.pass,false);
  assert.match(result.errors.join(" "),/not PASS/);
});
test("a PASS label without screenshot, reviewer, checks and matching file hash is rejected",()=>{
  const fake=passAudit();
  fake.review.cases[0].checks.footerVisible=false;
  fake.review.cases[1].screenshot="docs/private/unverified.png";
  fake.sourceBlobs[source]="b".repeat(40);
  const problems=validateVisualAudit(fake,proof).join(" ");
  assert.match(problems,/Git blob/);
  assert.match(problems,/footerVisible/);
  assert.match(problems,/in-repo PNG/);
});
test("approved audit with two screenshots and matching blobs passes the metadata gate",()=>{
  const result=checkVisualChanges([source,auditPath],()=>passAudit(),proof);
  assert.deepEqual(result.errors,[]);
  assert.equal(result.pass,true);
});
test("fresh UI modifications require their own audit coverage",()=>{
  const extra="egui-app/src/components.rs";
  const result=checkVisualChanges([source,extra,auditPath],()=>passAudit(),proof);
  assert.equal(result.pass,false);
  assert.match(result.errors.join(" "),/components.rs/);
});
test("non-UI changes do not create artificial visual approvals",()=>{
  const result=checkVisualChanges(["docs/README.md"],()=>{},proof);
  assert.equal(result.pass,true);
  assert.deepEqual(result.records,[]);
});

test("Rust-only test module edits do not require visual screenshot review",()=>{
  const before=[
    "pub fn render() {}",
    "#[cfg(test)]",
    "mod tests {",
    "    #[test]",
    "    fn contract() {",
    "        assert!(true);",
    "    }",
    "}",
  ].join("\n");
  const after=before.replace("assert!(true)","assert!(false)");
  const diff="@@ -6 +6 @@\n-        assert!(true);\n+        assert!(false);";
  assert.equal(isRustTestOnlyDiff(before,after,diff),true);
});
test("mixed production and test changes remain subject to visual review",()=>{
  const before=[
    "pub fn render() {}",
    "#[cfg(test)]",
    "mod tests {",
    "    #[test]",
    "    fn contract() {",
    "        assert!(true);",
    "    }",
    "}",
  ].join("\n");
  const after=before.replace("render() {}","render() { /* layout changed */ }")
    .replace("assert!(true)","assert!(false)");
  const diff="@@ -1 +1 @@\n-pub fn render() {}\n+pub fn render() { /* layout changed */ }\n@@ -6 +6 @@\n-        assert!(true);\n+        assert!(false);";
  assert.equal(isRustTestOnlyDiff(before,after,diff),false);
});
test("a newly added cfg(test) module is recognized as test-only",()=>{
  const before="pub fn render() {}";
  const after=before+"\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn contract() {}\n}";
  const diff="@@ -1,0 +2,5 @@\n+#[cfg(test)]\n+mod tests {\n+    #[test]\n+    fn contract() {}\n+}";
  assert.equal(isRustTestOnlyDiff(before,after,diff),true);
});
