import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const source = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("shared fade begins after readiness and never crosses a content identity", () => {
  const hook = source("src/shared/useReadyContentFade.ts");
  assert.match(hook, /if \(!ready\) return;/);
  assert.match(hook, /requestAnimationFrame\(\(\) => setRevealedKey\(contentKey\)\)/);
  assert.match(hook, /return \(\) => cancelAnimationFrame\(frame\)/);
  assert.match(hook, /revealedKey === contentKey && \(ready \|\| retainWhileRefreshing\)/);
  assert.doesNotMatch(hook, /setTimeout/);
});

test("checking panel preserves heading, scroll surface and empty guidance", () => {
  const desk = source("src/OrderDesk.tsx");
  const checks = source("src/StaffChecks.tsx");
  assert.match(desk, /const localChecksPhase = useReadyContentFade\([\s\S]*?staffChecksReady && localRecipientPhase === "ready", true/);
  assert.match(desk, /const checksFadePhase = props\.coordinatedFadePhase \?\? localChecksPhase/);
  assert.match(desk, /<StaffChecks[\s\S]*?fadePhase=\{checksFadePhase\}/);
  assert.match(checks, /<section className="staff-checks"[^>]*>[\s\S]*?<h3>/);
  assert.match(checks, /<small data-content-fade=\{fadePhase\}>/);
  assert.match(checks, /<div className="staff-check-scroll">/);
  assert.match(checks, /<p className="staff-empty">필수 확인 사항 없음<\/p>/);
  assert.match(checks, /className="staff-check-content" data-content-fade=\{fadePhase\}/);
  assert.doesNotMatch(checks, /className="staff-checks" data-content-fade/);
  assert.doesNotMatch(checks, /className="staff-check-scroll" data-content-fade/);
});

test("copy message header links options buttons and status never fade", () => {
  const msg = source("src/MessageWorkspace.tsx");
  const styles = source("src/styles/workflow-states.css");
  assert.match(msg, /const contentKey = \[order\.id, gid \?\? "", order\.orderType \?\? "", includeLinks\]/);
  assert.match(msg, /const bodyReady = Boolean\(gid && automatic\)/);
  assert.doesNotMatch(msg, /bodyReady = !gid/);
  assert.match(msg, /const localMessageFadePhase = useReadyContentFade\(contentKey, bodyReady\)/);
  assert.match(msg, /const messageFadePhase = sharedFadePhase \?\? localMessageFadePhase/);
  assert.match(msg, /<header className="pane-heading">/);
  assert.match(msg, /<div className="stage-message-preview">[\s\S]*?className="message-generated-content" data-content-fade=\{messageFadePhase\}/);
  assert.match(msg, /className="message-inline-status" role="status"/);
  assert.match(msg, /<footer className="message-copy-actions">/);
  assert.doesNotMatch(msg, /className="stage-message-preview" data-content-fade/);
  assert.match(styles, /\.message-generated-content \{[\s\S]*?min-height: 0;/);
});

test("a transient missing group never starts the auto-message fade-in", () => {
  const msg = source("src/MessageWorkspace.tsx");
  const hub = source("src/HubWorkspace.tsx");
  assert.match(msg, /const bodyReady = Boolean\(gid && automatic\)/);
  assert.match(msg, /resolved\?\.fingerprint === fingerprint &&/);
  assert.match(msg, /resolved\?\.groupId === gid && resolved\?\.links === includeLinks/);
  assert.match(hub, /gid=\{messageReady \? messageGroup : undefined\}/);
  assert.doesNotMatch(msg, /bodyReady = !gid/);
});

test("transient group or raw changes do not repeatedly reset message link options", () => {
  const msg = source("src/MessageWorkspace.tsx");
  assert.match(msg, /useLayoutEffect\(\(\) => \{\s*setStatus\(""\);\s*setIncludeLinks\(false\);\s*\}, \[order\.id\]\)/);
  assert.doesNotMatch(msg, /setIncludeLinks\(false\);\s*\}, \[order\.id, order\.raw, gid\]/);
});

test("raw-source heading, border and blank intake guidance never fade", () => {
  const hub = source("src/HubWorkspace.tsx");
  const desk = source("src/OrderDesk.tsx");
  const doc = source("src/SourceDocument.tsx");
  const css = source("src/styles/receipt-document.css");
  assert.match(hub, /sourceProjectionReady=\{sourceSettled\}/);
  assert.match(hub, /coordinatedFadePhase=\{sharedFadePhase\}/);
  assert.match(desk, /contentReady=\{props\.sourceProjectionReady \?\? true\}/);
  assert.match(doc, /const localSourceFadePhase = useReadyContentFade\(sourceKey, contentReady \|\| waitingForSource, true\)/);
  assert.match(doc, /const sourceFadePhase = coordinatedFadePhase \?\? localSourceFadePhase/);
  assert.match(doc, /<header className="pane-heading source-heading">/);
  assert.match(doc, /className=\{"source-document" \+ \(waitingForSource/);
  assert.match(doc, /<div className="source-intake-choice" aria-label="발주서 입력 방법">/);
  assert.match(doc, /className="source-raw-content" data-content-fade=\{sourceFadePhase\}/);
  assert.doesNotMatch(doc, /className="source-document" data-content-fade/);
  assert.match(css, /\.source-raw-content \{/);
  assert.match(css, /\.source-raw-content > \.linked-text-editor/);
});

test("common content opacity does not affect surrounding panel geometry", () => {
  const css = source("src/styles/receipt-layout.css");
  assert.match(css, /\.hub-panels \[data-content-fade="pending"\] \{[^}]*opacity: 0;[^}]*transition: none;/s);
  assert.match(css, /\.hub-panels \[data-content-fade="ready"\] \{[^}]*opacity: 1;[^}]*transition: opacity 260ms ease-out;/s);
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)/);
  assert.doesNotMatch(css, /\.hub-panels \.pane-heading\s*\{[^}]*opacity/);
});
