import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source = readFileSync(
  new URL("../src/orders/ManualClassification.tsx", import.meta.url), "utf8",
);
const section = (from, to) => {
  const a = source.indexOf(from);
  const b = source.indexOf(to, a + from.length);
  assert.ok(a >= 0 && b > a, "Missing source region: " + from);
  return source.slice(a, b);
};

test("manual recipient text is tracked as pending until its latest Rust reply settles", () => {
  const nativeCell = section('if (["name", "address", "phone", "mobile", "note"].includes(cell.field))', 'fail("지원하지 않는 수동 필드입니다."');
  assert.match(nativeCell, /manualTextRequest\.current\[key\] = request;\s*markNativeTextPending\(key, true\)/);
  assert.match(nativeCell, /action: "set_manual_recipient_field", model, cell, value/);
  assert.match(nativeCell, /manualTextRequest\.current\[key\] !== request/);
  assert.match(nativeCell, /\.catch\(\(e\) => \{[\s\S]*?fail\(e instanceof Error \? e\.message : String\(e\), cell\)/);
  assert.match(nativeCell, /\.finally\(\(\) => \{\s*\/\/ Superseded responses[\s\S]*?if \(manualTextRequest\.current\[key\] === request\)\s*markNativeTextPending\(key, false\)/);
  assert.match(source, /if \(busy\) nativeTextPending\.current\.add\(key\);\s*else nativeTextPending\.current\.delete\(key\)/);
});

test("a failed name/address/contact/note call cannot silently apply the older model", () => {
  assert.match(source, /const uncommitted = Object\.keys\(entry\)\.length > 0/);
  const footer = section('<footer className="manual-footer">', '{search && (');
  assert.match(footer, /\!delivery\.ready \|\| \!nativeOrderIssues\.ready \|\| nativeDraftBusy \|\| nativeTextBusy/);
  // The quantity eligibility is now a fingerprint-matched native Rust
  // projection. A pending or failed native check never enables Apply.
  assert.match(footer, /\|\| uncommitted \|\|[\s\S]*?\!quantityReady \|\| quantityStatus\?\.pending === true/);
  assert.match(footer, /if \(\!quantityReady \|\| quantityStatus\?\.pending === true\)/);
  assert.match(footer, /if \(\!requireNativeCellsSettled\(\) \|\| Object\.keys\(entry\)\.length > 0\)/);
  assert.doesNotMatch(footer, /pendingManualQuantity\(/);
  assert.match(source, /setEntry\(\(old\) => \{\s*const next = \{ \.\.\.old \};\s*delete next\[key\]/);
});

test("pending native cell operations cannot race structural edits, reset, undo, or product selection", () => {
  assert.match(source, /const requireNativeCellsSettled = \(\) => \{\s*if \(\!nativeDraftLock\.current && \!nativeTextPending\.current\.size &&[\s\S]*?\!manualQuantityPending\.current\.size\) return true/);
  assert.match(source, /const nativeStructure = async \(/);
  assert.match(source, /if \(\!requireNativeCellsSettled\(\)\) return false;\s*const previousModel = model;\s*nativeDraftLock\.current = true/);
  assert.match(source, /const cancelProduct = \(\) => \{\s*if \(\!requireNativeCellsSettled\(\)\) return/);
  assert.match(source, /const resize = \(n: number\) => \{\s*if \(\!requireNativeCellsSettled\(\)\) return/);
  assert.match(source, /const addProduct = \(recipientId: string\) => \{[\s\S]*?nativeStructure\("add_manual_item"/);
  assert.match(source, /const removeProduct = \(cell: Cell\) => \{[\s\S]*?nativeStructure\("remove_manual_item"/);
  assert.match(source, /const selectProduct = \([^]*?\) => \{\s*if \(\!allowed\(cell\) \|\| nativeDraftLock\.current \|\| \!requireNativeCellsSettled\(\)\) return/);
  assert.match(source, /const useBlock = \(id: string\) => \{\s*if \(nativeDraftLock\.current \|\| \!requireNativeCellsSettled\(\)\) return/);
  assert.match(source, /event\.key\.toLowerCase\(\) === "z"\) \{\s*event\.preventDefault\(\);\s*if \(\!requireNativeCellsSettled\(\)\) return/);
  assert.match(source, /title: "전체 초기화"/);
  assert.match(source, /disabled=\{nativeTextBusy \|\| nativeDraftBusy \|\| manualQuantityPending\.current\.size > 0\}/);
});
