import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const source = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("one workspace gate waits for source, message, checks and financial readiness", () => {
  const hub = source("src/HubWorkspace.tsx");
  assert.match(hub, /const fadeKey = \[\s*receptionOrder\.id,\s*detail\?\.id \?\? "",\s*visibleSelection\?\.shipmentId \?\? ""/);
  assert.match(hub, /if \(currentFadeKey\.current !== key\) return/);
  assert.match(hub, /const allSectionsReady = sourceSettled &&\s*fadeReadiness\.key === fadeKey &&\s*fadeReadiness\.message === true &&\s*fadeReadiness\.checks === true &&\s*fadeReadiness\.financial === true/);
  assert.match(hub, /useReadyContentFade\(fadeKey, allSectionsReady, true\)/);
  assert.match(hub, /const reportFadeReadiness = useCallback/);
});

test("all five content areas consume the SAME visible phase", () => {
  const hub = source("src/HubWorkspace.tsx");
  const desk = source("src/OrderDesk.tsx");
  const msg = source("src/MessageWorkspace.tsx");
  const src = source("src/SourceDocument.tsx");
  assert.match(hub, /<MessageWorkspace[\s\S]*?sharedFadePhase=\{sharedFadePhase\}/);
  assert.match(hub, /viewMode="reception"[\s\S]*?coordinatedFadePhase=\{sharedFadePhase\}/);
  assert.match(hub, /viewMode="details"[\s\S]*?coordinatedFadePhase=\{sharedFadePhase\}/);
  assert.match(desk, /const recipientRevealPhase = props\.coordinatedFadePhase \?\? localRecipientPhase/);
  assert.match(desk, /const checksFadePhase = props\.coordinatedFadePhase \?\? localChecksPhase/);
  assert.match(desk, /coordinatedFadePhase=\{props\.coordinatedFadePhase\}/);
  assert.match(src, /const sourceFadePhase = coordinatedFadePhase \?\? localSourceFadePhase/);
  assert.match(msg, /const messageFadePhase = sharedFadePhase \?\? localMessageFadePhase/);
  assert.match(desk, /fadePhase=\{checksFadePhase\}/);
  assert.match(desk, /data-recipient-fade=\{recipientRevealPhase\}/);
});

test("a new recipient does not reveal until checks and Rust financial previews have finished", () => {
  const desk = source("src/OrderDesk.tsx");
  const hook = source("src/orders/useOrderDesk.tsx");
  assert.match(desk, /onFadeReadiness\(props\.fadeKey, "checks", checksSettled\)/);
  assert.match(desk, /onFadeReadiness\(props\.fadeKey, "financial", recipientReady\)/);
  assert.match(desk, /const checksSettled = staffChecksReady \|\| Boolean\(staffChecksError\)/);
  assert.match(desk, /const recipientReady = !draft\.recipients\.length \|\|[\s\S]*?\(financialPreviewReady && \(delivery\.ready \|\| Boolean\(delivery\.error\)\)\)/);
  assert.match(hook, /financialPreviewReady: flowSummaryReady && nativeLines\.ready &&/);
});

test("message readiness is for the current order, group, fingerprint and option only", () => {
  const msg = source("src/MessageWorkspace.tsx");
  const hub = source("src/HubWorkspace.tsx");
  assert.match(msg, /resolved\?\.fingerprint === fingerprint &&\s*resolved\?\.orderId === order\.id &&/);
  assert.match(msg, /resolved\?\.groupId === gid && resolved\?\.links === includeLinks/);
  assert.match(msg, /loadError\?\.fingerprint === fingerprint &&\s*loadError\.groupId === gid && loadError\.links === includeLinks/);
  assert.match(msg, /const messageSettled = projectionReady &&\s*\(bodyReady \|\| noMessageExpected \|\| Boolean\(currentLoadError \|\| projectionError\)\)/);
  assert.match(msg, /onFadeReadiness\(fadeKey, "message", messageSettled\)/);
  assert.match(hub, /noMessageExpected=\{noMessageExpected\}/);
  assert.doesNotMatch(msg, /bodyReady = !gid/);
});

test("the five permanent section frames, empty guidance and action buttons never fade", () => {
  const checks = source("src/StaffChecks.tsx");
  const sourceDoc = source("src/SourceDocument.tsx");
  const msg = source("src/MessageWorkspace.tsx");
  const desk = source("src/OrderDesk.tsx");
  assert.doesNotMatch(checks, /className="staff-checks" data-content-fade/);
  assert.doesNotMatch(sourceDoc, /className="source-document" data-content-fade/);
  assert.doesNotMatch(msg, /className="stage-message-preview" data-content-fade/);
  assert.doesNotMatch(desk, /className="recipient-body" data-recipient-fade/);
  assert.doesNotMatch(desk, /className="items-freight-panel" data-recipient-fade/);
  assert.match(checks, /필수 확인 사항 없음/);
  assert.match(sourceDoc, /className="source-intake-choice"/);
  assert.match(msg, /className="message-copy-actions"/);
});
