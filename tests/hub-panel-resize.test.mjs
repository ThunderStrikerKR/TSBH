import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { tsImport } from "tsx/esm/api";

const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");
const { defaultPanels, panelMinimumWidths, resizePanelRatios } =
  await tsImport("../src/HubPanels.tsx", import.meta.url);

const toPixels = (ratios, totalWidth) =>
  ratios.map((ratio) => ratio * totalWidth / ratios.reduce((sum, v) => sum + v, 0));

test("all panel ratios are used and drag actions persist through HubWorkspace settings", () => {
  const panels = read("src/HubPanels.tsx");
  const hub = read("src/HubWorkspace.tsx");
  assert.doesNotMatch(panels, /r = \[validPanels\(ratios\)\[0\], 1, 1, 1\]/);
  assert.match(panels, /const currentRatios = validPanels\(previewRatios \?\? ratios\)/);
  assert.match(panels, /onPointerDown=\{\(event\) =>/);
  assert.match(panels, /onPointerMove=\{\(event\) =>/);
  assert.match(panels, /onPointerUp=\{\(event\) =>/);
  assert.match(panels, /onKeyDown=\{\(event\) =>/);
  assert.match(panels, /onSave\(state\.current\)/);
  assert.match(hub, /settings: \{ \.\.\.state\.settings, panelRatios: ratios \}/);
  assert.match(panels, /onDoubleClick=\{\(\) => \{/);
  assert.match(panels, /onReset\(\)/);
});

test("drag changes adjacent panels only, retaining the 440px single-row workflow minimum", () => {
  const minimum = panelMinimumWidths;
  assert.deepEqual(minimum, [200, 250, 250, 440]);
  const widths = [260, 340, 350, 460];
  const result = resizePanelRatios(defaultPanels, [0,1,2,3], widths, 2, 100, minimum);
  const pixels = toPixels(result, widths.reduce((a,b) => a+b,0));
  assert.ok(Math.abs(pixels[0] - 260) < 0.001);
  assert.ok(Math.abs(pixels[1] - 340) < 0.001);
  assert.ok(Math.abs(pixels[2] - 370) < 0.001);
  assert.ok(Math.abs(pixels[3] - 440) < 0.001);
  assert.ok(Math.abs(result.reduce((sum, n) => sum + n, 0) - 100) < 0.001);
  assert.deepEqual(resizePanelRatios(defaultPanels, [0,1,2,3], widths, 2, 0, minimum),
    defaultPanels, "no movement must not modify persisted layout");
});

test("other dragged splitters preserve non-adjacent widths", () => {
  const widths = [260, 340, 350, 460];
  const output = resizePanelRatios(defaultPanels, [0,1,2,3],
    widths, 1, -90, panelMinimumWidths);
  const pixels = toPixels(output, widths.reduce((a,b)=>a+b,0));
  for (const [i, expected] of [260,250,440,460].entries())
    assert.ok(Math.abs(pixels[i] - expected) < 0.001,
      `panel ${i} expected ${expected}, got ${pixels[i]}`);
});

test("external Kakao mode retains the hidden panel ratio after resizing", () => {
  const widths = [330, 380, 470];
  const output = resizePanelRatios(defaultPanels, [1,2,3],
    widths, 1, 35, panelMinimumWidths);
  const ratio = output[0] / output.reduce((a,b)=>a+b,0);
  assert.ok(Math.abs(ratio - 0.20) < 0.001);
  assert.ok(output[3] > 0);
});

test("single-line dealer name and fixed stage lights are enforced before allowing right-panel narrowing", () => {
  const css = read("src/styles/workflow.css");
  assert.match(css, /\.workflow-group > header\s*\{[\s\S]*?flex-wrap: nowrap;/);
  assert.match(css, /\.workflow-group \.flow-group-name\s*\{[\s\S]*?flex: 0 0 146px;[\s\S]*?width: 146px;/);
  assert.match(css, /\.flow-heading-toggle > \.workflow-signals\s*\{[\s\S]*?flex: 0 0 90px;[\s\S]*?width: 90px;/);
  assert.match(css, /\.workflow-group \.workflow-signals \.signal\s*\{[\s\S]*?width: 18px;[\s\S]*?height: 18px;/);
  assert.match(css, /\.workflow-group > header > button:not\(\.flow-heading-toggle\)\s*\{[\s\S]*?flex: 0 0 auto;/);
});
