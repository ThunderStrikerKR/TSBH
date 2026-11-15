// Complete native GUI navigation audit of the *current* CI-built Tauri EXE.
// No mocked Tauri IPC or Vite browser; Windows WebView2 is controlled through CDP.
// A synthetic baseline is created exclusively on a disposable GitHub Windows runner.
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { promises as fs } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { setTimeout as sleep } from "node:timers/promises";
import { chromium } from "playwright";

assert.equal(process.platform, "win32", "Real Windows GUI audit only");
assert.equal(process.env.GITHUB_ACTIONS, "true", "Use only a disposable CI runner");
const exe = resolve(process.env.BANDO_NATIVE_EXE || "");
assert.match(exe, /\.exe$/i);
await fs.access(exe);
const out = resolve(process.env.BANDO_NATIVE_OUT || "final-windows-screen-evidence");
await fs.mkdir(out, { recursive: true });
const root = await fs.mkdtemp(join(tmpdir(), "bando-screen-audit-"));
const port = 19651;
const master = resolve(process.env.APPDATA || "",
  "com.bandostand.bandohub.tauri", "bando-hub-v001.sqlite");
assert.ok(process.env.APPDATA, "APPDATA must be present");
await assert.rejects(fs.access(master), { code: "ENOENT" },
  "Do not run on an existing operational database");

const report = {
  type: "final-real-Windows-Tauri-EXE-screen-audit",
  sourceCommit: process.env.BANDO_NATIVE_BIN_COMMIT || "",
  automationCommit: process.env.GITHUB_SHA || "",
  binary: basename(exe),
  usesMockIPC: false,
  isolatedTestDatabase: true,
  startedAt: new Date().toISOString(),
  required: { menus: 8, settingsTabs: 5, dialogs: 4 },
  passes: [], captures: [], errors: [], status: "STARTED"
};
const save = () => fs.writeFile(join(out, "screen-audit.json"),
  JSON.stringify(report, null, 2), "utf8");
await save();
const record = (type, name, details = {}) => {
  report.passes.push({ type, name, ...details });
  console.log("FULL_NATIVE_GUI_PASS", JSON.stringify({ type, name, ...details }));
};
let app, browser, page, appErr = "", appOut = "";
const killTree = (pid) => {
  if (!pid) return;
  try { execFileSync("taskkill.exe", ["/PID", String(pid), "/T", "/F"],
    { stdio: "pipe", timeout: 16000 }); }
  catch { /* A process already exited or has no children. */ }
};
const waitForFile = async (path, process, label) => {
  for (let i = 0; i < 130; i++) {
    if (process.exitCode !== null) throw Error(label + " EXE exited early");
    try { if ((await fs.stat(path)).size > 0) return; } catch {}
    await sleep(300);
  }
  throw Error(label + " SQLite was not created");
};
const visible = async (locator) => {
  await locator.waitFor({ state: "visible", timeout: 25000 });
  return locator;
};
const capture = async (kind, name) => {
  await page.waitForTimeout(170);
  const metric = await page.evaluate(() => {
    const vis = el => {
      const r = el.getBoundingClientRect();
      const s = getComputedStyle(el);
      return r.width > 0 && r.height > 0 && s.display !== "none" &&
        s.visibility !== "hidden";
    };
    const rect = el => {
      const r = el.getBoundingClientRect();
      return { x: +r.x.toFixed(1), y: +r.y.toFixed(1),
        width: +r.width.toFixed(1), height: +r.height.toFixed(1) };
    };
    return {
      viewport: { width: innerWidth, height: innerHeight },
      documentWidth: document.documentElement.scrollWidth,
      h1: [...document.querySelectorAll("h1")].filter(vis).map(el => el.textContent.trim()),
      activeTopNav: [...document.querySelectorAll("nav[aria-label='업무 메뉴'] button[aria-current='page']")]
        .map(el => el.textContent.trim()),
      dialogs: [...document.querySelectorAll("dialog[open]")].filter(vis)
        .map(el => ({ name: el.getAttribute("aria-label"), rect: rect(el) })),
      headings: [...document.querySelectorAll("h2,h3")].filter(vis).slice(0,14)
        .map(el => ({ text: el.textContent.trim().slice(0,110), rect: rect(el) })),
      overflowingRegions: [...document.querySelectorAll(".management-scroll,.table-scroll,.price-table-scroll,.shipping-table-scroll")]
        .filter(vis).map(el => ({
          className: String(el.className), scrollWidth: el.scrollWidth,
          clientWidth: el.clientWidth, scrollHeight: el.scrollHeight,
          clientHeight: el.clientHeight
        }))
    };
  });
  const namePart = (kind + "-" + name).replace(/[^\p{L}\p{N}-]+/gu, "-").slice(0,100);
  const file = String(report.captures.length + 1).padStart(2, "0") + "-" + namePart + ".png";
  await page.screenshot({ path: join(out, file), animations: "disabled", timeout: 30000 });
  report.captures.push({ kind, name, file, ...metric });
  console.log("FULL_NATIVE_GUI_SCREEN", JSON.stringify({
    kind, name, file, h1: metric.h1, dialogs: metric.dialogs.map(x => x.name)
  }));
  await save();
};
const nav = () => page.locator("nav[aria-label='업무 메뉴']");
const openMenu = async name => {
  await nav().getByRole("button", { name, exact: true }).click({ timeout: 30000 });
  await page.waitForFunction(label => {
    const selected = document.querySelector("nav[aria-label='업무 메뉴'] button[aria-current='page']");
    return selected?.textContent?.trim() === label;
  }, name, { timeout: 30000 });
  if (name === "접수") {
    await visible(page.locator(".source-pane"));
  } else {
    await visible(page.getByRole("heading", { name, exact: true }).first());
  }
  const unexpected = page.locator(".screen-boundary-error");
  assert.equal(await unexpected.count(), 0, "React error boundary after " + name);
  record("menu", name);
  await capture("menu", name);
};
const openSubtab = async (menu, name, locator, check) => {
  await visible(locator);
  await locator.click({ timeout: 30000 });
  if (check) await check(locator);
  record("subtab", menu + " / " + name);
  await capture("subtab", menu + "-" + name);
};
const captureDialog = async (name, trigger) => {
  await trigger();
  const dialog = page.locator("dialog[open]").last();
  await dialog.waitFor({ state: "visible", timeout: 30000 });
  const metrics = await dialog.evaluate(el => {
    const r = el.getBoundingClientRect(), inside = document.activeElement;
    const cx = r.left + Math.min(r.width / 2, innerWidth / 2);
    const cy = r.top + Math.min(r.height / 2, innerHeight / 2);
    const hit = document.elementFromPoint(cx, cy);
    return {
      open: el.open, modal: el.getAttribute("aria-modal"),
      name: el.getAttribute("aria-label"),
      bounds: { x:r.x,y:r.y,width:r.width,height:r.height },
      viewport: { width:innerWidth,height:innerHeight },
      topLayerHit: !!hit && (el === hit || el.contains(hit)),
      focusWithin: !!inside && el.contains(inside)
    };
  });
  assert.ok(metrics.open && metrics.modal === "true",
    name + " is not a native HTML dialog");
  assert.ok(metrics.topLayerHit, name + " is obscured by another UI layer");
  assert.ok(metrics.bounds.width > 200 && metrics.bounds.height > 100,
    name + " has invalid dialog geometry");
  record("dialog", name, metrics);
  await capture("dialog", name);
  await dialog.getByRole("button", { name: "닫기 ×", exact: true }).click();
  await dialog.waitFor({ state: "detached", timeout: 20000 });
  record("dialog-close", name);
  assert.equal(await page.locator("dialog[open]").count(), 0,
    name + " left a modal in top layer after close");
};

try {
  let bootstrap = spawn(exe, [], {
    cwd: dirname(exe), windowsHide: false, stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, WEBVIEW2_USER_DATA_FOLDER: join(root, "bootstrap-webview2") }
  });
  try { await waitForFile(master, bootstrap, "Synthetic baseline"); await sleep(1200); }
  finally { killTree(bootstrap.pid); await sleep(1000); }
  app = spawn(exe, ["--bando-developer-test"], {
    cwd: dirname(exe), windowsHide: false, stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, WEBVIEW2_USER_DATA_FOLDER: join(root, "gui-webview2") }
  });
  app.stdout?.on("data", b => appOut += b.toString());
  app.stderr?.on("data", b => appErr += b.toString());
  let target;
  for (let n = 0; n < 175; n++) {
    if (app.exitCode !== null) throw Error("Native EXE exited: " + appErr.slice(-1000));
    try {
      const res = await fetch("http://127.0.0.1:" + port + "/json/list",
        { signal: AbortSignal.timeout(900) });
      if (res.ok) {
        const data = await res.json();
        target = data.find(x => x.type === "page" &&
          /tauri|localhost/i.test(x.url || ""));
        if (target) break;
      }
    } catch {}
    await sleep(350);
  }
  assert.ok(target, "WebView2 CDP unavailable: no mock/fallback accepted");
  browser = await chromium.connectOverCDP("http://127.0.0.1:" + port,
    { timeout: 30000 });
  const context = browser.contexts()[0];
  page = context?.pages().find(p => /tauri|localhost/i.test(p.url())) ??
    context?.pages()[0];
  assert.ok(page, "No real native WebView2 page");
  page.setDefaultTimeout(30000);
  page.on("pageerror", error => report.errors.push("pageerror: " + error.message));
  page.on("console", msg => {
    if (msg.type() === "error") report.errors.push("console-error: " + msg.text());
  });
  await visible(nav());
  const startup = await page.evaluate(async () => {
    return {
      href: location.href,
      hasIPC: !!window.__TAURI_INTERNALS__?.invoke,
      hasBando: !!window.bando,
      testWorkspace: await window.bando?.workspaceMode(),
      startup: await window.bando?.startupStatus(),
      state: await window.bando?.load()
    };
  });
  assert.ok(startup.hasIPC && startup.hasBando && startup.testWorkspace === true,
    "Real Rust Tauri IPC and isolated developer TEST DB required");
  assert.equal(startup.startup.phase, "READY");
  assert.ok(/^http:\/\/tauri\.localhost/.test(startup.href), "Not a Tauri WebView2 URL");
  report.native = { href: startup.href, testWorkspace: startup.testWorkspace,
    initialRevision: startup.state.revision, clientCount: startup.state.clients.length,
    productCount: startup.state.products.length };
  assert.ok(startup.state.products.length > 0,
    "Product catalog missing; product dialogs could not be exercised");
  record("native", "actual Windows EXE and isolated Rust IPC", report.native);

  const menus = ["접수", "주문 내역", "엑셀 관리", "일일 결산",
    "상품 관리", "운임 관리", "거래처 관리", "설정"];
  const subtabGroups = {
    "엑셀 관리": ["공장 엑셀 대기", "매장 발송 대기", "엑셀 작성 이력"],
    "상품 관리": ["상품 목록", "카테고리 관리", "모델군 관리", "별칭 관리", "브랜드 관리"],
    "운임 관리": ["상품별 기본 운임", "동급 배송 그룹 지정",
      "동급 그룹 운임표", "조합 / 기준 이력"],
    "거래처 관리": ["거래처 목록", "자동 메시지 편집"]
  };
  for (const menu of menus) {
    await openMenu(menu);
    if (menu === "설정") {
      for (const title of ["기본 설정", "파일 관리", "데이터 관리",
        "시스템", "개발자 모드"]) {
        const tab = page.getByRole("tab", { name: title, exact: true });
        await openSubtab(menu, title, tab, async () => {
          await assert.doesNotReject(() =>
            page.waitForFunction(id => {
              const active = document.querySelector("[role='tab'][aria-selected='true']");
              return active?.textContent?.trim() === id &&
                !!document.querySelector("[role='tabpanel']");
            }, title, {timeout: 22000}));
        });
        record("settings-tab", title);
      }
    } else if (subtabGroups[menu]) {
      for (const title of subtabGroups[menu]) {
        let selector;
        if (menu === "엑셀 관리") selector = page.locator(".workflow-queue-tabs")
          .getByRole("button", { name: title, exact: true });
        else if (menu === "상품 관리") selector = page.locator("nav[aria-label='상품 관리 메뉴']")
          .getByRole("button", { name: title, exact: true });
        else if (menu === "운임 관리") selector = page.locator("[aria-label='배송 관리']")
          .getByRole("button", { name: title, exact: true });
        else selector = page.locator("nav[aria-label='거래처 관리 메뉴']")
          .getByRole("button", { name: title, exact: true });
        await openSubtab(menu, title, selector, async el => {
          await page.waitForFunction(({ label, type }) => {
            const container =
              type === "엑셀 관리" ? ".workflow-queue-tabs" :
              type === "상품 관리" ? "nav[aria-label='상품 관리 메뉴']" :
              type === "운임 관리" ? "[aria-label='배송 관리']" :
              "nav[aria-label='거래처 관리 메뉴']";
            const pressed = [...document.querySelectorAll(container + " button")]
              .find(b => b.textContent?.trim() === label);
            return pressed?.getAttribute(type === "상품 관리" || type === "거래처 관리"
              ? "aria-current" : "aria-pressed") ===
              (type === "상품 관리" || type === "거래처 관리" ? "page" : "true");
          }, {label:title,type:menu}, {timeout: 20000});
        });
        if (menu === "거래처 관리" && title === "자동 메시지 편집") {
          for (const nested of ["상황별 공통 안내", "잠정 상품 선택 안내"]) {
            const inner = page.locator("nav[aria-label='자동 메시지 편집 종류']")
              .getByRole("button", { name: nested, exact: true });
            await openSubtab(menu, nested, inner, async el => {
              await page.waitForFunction(label =>
                [...document.querySelectorAll("nav[aria-label='자동 메시지 편집 종류'] button")]
                  .some(b => b.textContent?.trim() === label &&
                    b.getAttribute("aria-current") === "page"),
                nested, {timeout: 15000});
            });
          }
        }
      }
    }
  }

  // Verify the largest four real editing dialogs and their top-layer hit-test.
  // No Save / Delete / Confirm action is ever pressed by this navigation audit.
  await openMenu("상품 관리");
  await captureDialog("상품 상세 관리", async () => {
    await visible(page.locator(".product-prices tbody button").first());
    await page.locator(".product-prices tbody").getByRole("button",
      { name: "상세 관리", exact: true }).first().click();
  });
  await captureDialog("상품 등록", async () => {
    await page.getByRole("button", { name: "상품 추가", exact: true }).click();
    await page.getByRole("button", { name: "직접 등록", exact: true }).click();
  });

  await openMenu("거래처 관리");
  await captureDialog("거래처 등록", async () => {
    await page.getByRole("button", { name: "＋ 거래처 등록", exact: true }).click();
  });

  await openMenu("운임 관리");
  await visible(page.locator(".shipping-product-list"));
  const freightTrigger = page.getByRole("button",
    { name: "기본 운임 설정", exact: true }).first();
  const groupedTrigger = page.getByRole("button",
    { name: "그룹 기준 편집", exact: true }).first();
  await page.waitForFunction(() =>
    [...document.querySelectorAll("button")].some(b =>
      ["기본 운임 설정", "그룹 기준 편집"].includes(b.textContent?.trim()) &&
      !b.disabled), null, {timeout: 30000});
  await captureDialog("상품/그룹 기본 운임 설정", async () => {
    if (await freightTrigger.count()) await freightTrigger.click();
    else await groupedTrigger.click();
  });
  // This is the fourth modal, irrespective of whether the selected product
  // uses its own freight or the shared shipping-group tariff.
  const dialogCount = report.passes.filter(x => x.type === "dialog").length;
  assert.equal(report.passes.filter(x => x.type === "menu").length, 11,
    "Eight base menus plus three modal revisit entries must be captured");
  assert.equal(report.passes.filter(x => x.type === "settings-tab").length, 5);
  assert.equal(dialogCount, 4, "Four native editing dialogs must be verified");
  const finalState = await page.evaluate(async () => window.bando.load());
  assert.equal(finalState.revision, startup.state.revision,
    "Screen-only audit must not write to TEST SQLite");
  assert.deepEqual(finalState.orders, startup.state.orders,
    "Screen-only audit must preserve all TEST orders");
  assert.equal(report.errors.length, 0,
    "Browser errors during native screen navigation: " + report.errors.join(" | "));
  report.status = "REAL_WINDOWS_EIGHT_MENUS_FIVE_SETTINGS_FOUR_MODALS_PASSED";
  record("preservation", "No SQLite revision or order mutation", {
    revision: finalState.revision
  });
  console.log("FULL_NATIVE_GUI_AUDIT_PASS", JSON.stringify({
    menus: 8, settingsTabs: 5, dialogs: 4,
    subtabVisits: report.passes.filter(x => x.type === "subtab").length,
    screenshots: report.captures.length, binaryCommit: report.sourceCommit
  }));
} catch (error) {
  report.status = "FAILED";
  report.failure = error?.stack || String(error);
  process.exitCode = 1;
  console.error("FULL_NATIVE_GUI_AUDIT_FAILED", report.failure);
  if (page) {
    try { await capture("failure", "diagnostic"); } catch {}
  }
} finally {
  report.stderrTail = appErr.slice(-2500);
  report.stdoutTail = appOut.slice(-2500);
  await save();
  await browser?.close().catch(() => {});
  killTree(app?.pid);
}
