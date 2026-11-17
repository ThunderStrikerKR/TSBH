import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { tsImport } from "tsx/esm/api";

const read = (path) => readFileSync(new URL("../" + path, import.meta.url), "utf8");
const { defaultPanels, panelMinimumWidths, resizePanelRatios, validPanels } =
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

test("drag changes adjacent panels only, retaining the 420px single-row workflow minimum", () => {
  const minimum = panelMinimumWidths;
  assert.deepEqual(minimum, [200, 250, 250, 420]);
  const widths = [260, 340, 350, 460];
  const result = resizePanelRatios(defaultPanels, [0,1,2,3], widths, 2, 100, minimum);
  const pixels = toPixels(result, widths.reduce((a,b) => a+b,0));
  assert.ok(Math.abs(pixels[0] - 260) < 0.001);
  assert.ok(Math.abs(pixels[1] - 340) < 0.001);
  assert.ok(Math.abs(pixels[2] - 390) < 0.001);
  assert.ok(Math.abs(pixels[3] - 420) < 0.001);
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
  assert.match(css, /\.workflow-group \.flow-group-name\s*\{[\s\S]*?flex: 0 0 162px;[\s\S]*?width: 162px;/);
  assert.match(css, /\.flow-heading-toggle > \.workflow-signals\s*\{[\s\S]*?flex: 0 0 82px;[\s\S]*?width: 82px;/);
  assert.match(css, /\.workflow-group \.workflow-signals \.signal\s*\{[\s\S]*?width: 18px;[\s\S]*?height: 18px;/);
  assert.match(css, /\.workflow-group > header > button:not\(\.flow-heading-toggle\)\s*\{[\s\S]*?flex: 0 0 auto;/);
});

test("legacy equal workflow columns migrate to the compact 22 percent default", () => {
  const source = read("src/HubPanels.tsx");
  assert.match(source, /defaultPanels = \[20, 29, 29, 22\]/);
  assert.match(source, /const oldDefault = 80 \/ 3/);
  assert.match(source, /panelMinimumWidths = \[200, 250, 250, 420\]/);
});

test("1920px workspace spans all available width using raw fr weights, not fractions smaller than one", () => {
  const panels = read("src/HubPanels.tsx");
  const outer = read("src/HubShell.tsx");
  const shell = read("src/styles/shell.css");
  const receipt = read("src/styles/receipt-layout.css");
  const app = read("src/App.tsx");
  // Fractions below 1fr plus a fixed 420px minimum can leave unused space.
  // Raw 29fr/29fr/22fr weights distribute all available horizontal room.
  assert.ok(panels.includes("minmax(" + "$" + "{minima[i]}px," + "$" + "{currentRatios[i]}fr)"));
  assert.doesNotMatch(panels, /currentRatios\[i\]\s*\/\s*total/);
  assert.match(panels, /width: "100%",\s*gridTemplateColumns/);
  assert.match(shell, /\.hub-shell-scroll\s*\{[\s\S]*?width: 100%;/);
  assert.match(shell, /\.hub-shell\s*\{[\s\S]*?width: 100%;/);
  assert.match(shell, /\.hub-workspace-scroll\s*\{[\s\S]*?width: 100%;/);
  assert.match(shell, /\.hub-panels\s*\{[\s\S]*?width: 100%;/);
  assert.match(shell, /\.hub-desk-host\s*\{[\s\S]*?width: 100%;/);
  assert.match(receipt, /\.hub-live-workspace\s*\{[\s\S]*?width: 100%;/);
  assert.match(outer, /mainMinimum = \(panelMinimumWidths\[1\] \+ panelMinimumWidths\[2\] \+ panelMinimumWidths\[3\] \+ 12\) \* viewportScale/);
  assert.match(app, /ratios=\{state\.settings\.panelRatios \?\? defaultPanels\}/);
  assert.match(app, /panelRatios: defaultPanels/);
});

test("both historic preset layouts normalize to 20:29:29:22 while custom sizes persist", () => {
  assert.deepEqual(defaultPanels, [20, 29, 29, 22]);
  assert.deepEqual(validPanels([20, 80/3, 80/3, 80/3]), defaultPanels);
  assert.deepEqual(validPanels([20, 30, 30, 20]), defaultPanels);
  assert.deepEqual(validPanels([20, 25, 35, 20]), [20, 25, 35, 20]);
  assert.deepEqual(validPanels(defaultPanels), defaultPanels);
});
