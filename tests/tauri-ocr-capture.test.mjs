import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read = (p) => readFileSync(new URL("../" + p, import.meta.url), "utf8");
const source = read("src/SourceDocument.tsx");
const bridge = read("src/tauri-bridge.ts");
const lib = read("src-tauri/src/lib.rs");
const ocr = read("src-tauri/src/ocr.rs");
const script = read("src-tauri/resources/ocr/recognize.py");
test("blank source canvas groups clipboard, alternative input, and folder guidance without moving client card", () => {
  const css=read("src/styles/receipt-document.css");
  const hub=read("src/HubWorkspace.tsx");
  for (const label of ["대리발송", "대리점 납품", "직접 입력", "캡처 범위 지정"])
    assert.ok(source.includes(label), label);
  assert.match(source, /className="source-new-order"/);
  assert.match(source, /onClick=\{onNew\}/);
  assert.match(source, /className="source-intake-choice"/);
  assert.match(source, /waitingForSource \? \(/);
  for (const label of ["클립보드 접수", "다른 방법으로 접수", "폴더 자동 인식"]) assert.ok(source.includes(label));
  assert.doesNotMatch(source, /source-intake-half/);
  assert.ok(source.indexOf('className="source-top"') < source.indexOf('className={"source-document"'));
  for (const key of ["Ctrl", "Shift", "S", "Print Screen"]) {
    assert.ok(source.includes(`<kbd className="source-intake-keycap">${key}</kbd>`));
  }
  assert.doesNotMatch(source, /캡처 앱 설정/);
  assert.match(css, /\.source-intake-keycap\s*\{[^}]*border:/);
  assert.match(css, /\.source-intake-other \.source-intake-buttons button\s*\{[^}]*background:/);
  assert.match(source, /onClick=\{\(\) => void paste\(undefined, "AGENCY"\)\}/);
  assert.match(source, /onClick=\{\(\) => void paste\(undefined, "DEALER"\)\}/);
  assert.match(source, /onClick=\{\(\) => setRetail\(true\)\}/);
  assert.match(source, /onClick=\{\(\) => void captureOcr\(\)\}/);
  assert.doesNotMatch(source, /source-heading-actions/);
  assert.match(css, /\.source-intake-choice\s*\{[^}]*display: flex/);
  assert.match(css, /\.source-intake-buttons\s*\{[^}]*flex-wrap: wrap/);
  assert.match(css, /width: auto/);
  assert.doesNotMatch(css, /\.source-intake-half/);
  assert.doesNotMatch(source, /data-ui-role="action-quiet"/);
  assert.match(source, /onClick=\{\(\) => setRetail\(true\)\}>직접 입력<\/button>/);
  assert.match(source, /onClick=\{\(\) => void captureOcr\(\)\}>캡처 범위 지정<\/button>/);
  assert.match(hub, /onNew=\{createReceptionLive\}/);
});
test("dedicated image inbox scans stable files and uses existing review dialog", () => {
  const inbox = read("src-tauri/src/order_image_inbox.rs");
  const settings = read("src/SettingsPanel.tsx");
  assert.match(inbox, /order_image_watch_scan/);
  assert.match(inbox, /order_image_ocr/);
  assert.match(inbox, /observations >= 3/);
  assert.match(settings, /발주 이미지 자동 수신 폴더/);
  assert.match(source, /setOcrResult\(capture\)/);
  assert.match(bridge, /order_image_watch_scan/);
  assert.match(source, /state\.settings\.orderImageInboxDirectory/);
});
test("OCR result requires user review before replacing a provisional order", () => {
  assert.match(source, /confirmAction\("기존 미확정 원문/);
  assert.match(source, /await nativeReplaceReception\(order, raw\)/);
  assert.match(source, /action: "replace_reception"/);
  assert.match(source, /latestReception\.current\.order\.id !== order\.id/);
  assert.match(source, /latestReception\.current\.order\.raw !== order\.raw/);
  assert.match(source, /latestReception\.current\.text !== text/);
  assert.match(source, /setOcrResult\(null\)/);
  assert.match(source, /if \(formal\) return/);
  assert.doesNotMatch(ocr, /state_save/);
});
test("Windows local PP-OCRv5 uses new clipboard images only", () => {
  assert.match(lib, /ocr::capture_ocr/);
  assert.match(bridge, /invoke<OcrCapture>\("capture_ocr"\)/);
  assert.match(ocr, /GetClipboardSequenceNumber/);
  assert.match(ocr, /pub async fn clipboard_ocr/);
  assert.match(source, /window\.bando\.clipboardOcr\(\)/);
  assert.match(bridge, /invoke<OcrCapture>\("clipboard_ocr"\)/);
  assert.ok(ocr.includes('include_str!("../resources/ocr/recognize.py")'));
  assert.match(script, /ocr_version="PP-OCRv5"/);
  assert.match(script, /lang="korean"/);
});

test("OCR progress is shown prominently over the source body and never replaces button text", () => {
  const css = read("src/styles/receipt-document.css");
  assert.match(source, /const \[ocrPhase, setOcrPhase\] = useState/);
  assert.match(source, /setOcrPhase\("selecting"\)/);
  assert.match(source, /setOcrPhase\("recognizing"\)/);
  assert.match(source, /setOcrPhase\("idle"\)/);
  assert.match(source, /\{ocrPhase === "recognizing" && \(/);
  assert.doesNotMatch(source, /\{ocrPhase === "selecting" && \([\s\S]*?<strong>OCR 처리 중/);
  assert.match(source, /className="source-ocr-progress"/);
  assert.match(source, /<strong>OCR 처리 중…<\/strong>/);
  assert.match(source, /onClick=\{\(\) => void captureOcr\(\)\}>캡처 범위 지정<\/button>/);
  assert.doesNotMatch(source, /\{busy \? "처리 중…"/);
  assert.match(css, /\.source-pane \.source-ocr-progress\s*\{[\s\S]*?position: absolute/);
});

test("Esc cancellation is silent, restores BANDO window and never starts PaddleOCR", () => {
  assert.match(ocr, /GetAsyncKeyState\(0x1B\)/);
  assert.match(ocr, /escape_state & 0x0001/);
  assert.match(ocr, /if esc_released && \(escape_pressed \|\| escape_tapped\) \{/);
  assert.match(ocr, /return Err\(OCR_CANCELLED\.into\(\)\)/);
  assert.match(ocr, /CaptureWindowGuard::minimize\(&app\)/);
  assert.match(ocr, /impl Drop for CaptureWindowGuard/);
  assert.match(ocr, /window\.unminimize\(\)/);
  assert.match(ocr, /window\.set_focus\(\)/);
  assert.match(ocr, /screen_snip::snip\(\)\?/);
  assert.match(source, /if \(detail !== "OCR_CANCELLED"\)/);
  assert.match(source, /stopListening\?\.\(\)/);
});

test("recognition starts after screenshot selection and its worker is cancellable", () => {
  assert.match(lib, /ocr::capture_ocr, ocr::clipboard_ocr, ocr::cancel_ocr/);
  assert.match(bridge, /invoke<void>\("cancel_ocr"\)/);
  assert.match(ocr, /let mut child = command\.spawn\(\)/);
  assert.match(ocr, /app\.emit\("bando-ocr-phase", "recognizing"\)/);
  assert.match(ocr, /if OCR_CANCEL_REQUESTED\.load\(Ordering::Acquire\) \{[\s\S]*?child\.kill\(\)/);
  assert.match(source, /listen<string>\("bando-ocr-phase"/);
  assert.match(source, /event\.payload === "recognizing"/);
  assert.match(source, /window\.bando\?\.cancelOcr\?\.\(\)/);
  assert.match(source, /<strong>OCR 처리 중…<\/strong>/);
});
