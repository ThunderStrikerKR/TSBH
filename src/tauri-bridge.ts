/** BANDO HUB Tauri bridge. Active React actions map to native commands.
 * Planned mobile networking remains explicitly unavailable offline.
 */
import { invoke } from "@tauri-apps/api/core";
import { coalescePureNativeRead } from "./shared/native-read-coalescing";
import { auditStateTransition } from "./perf-audit";

/** CI-only native call audit. Vite drops this instrumentation from release builds. */
function auditNativeCall(
  command: string,
  meta: { domain?: string; operation?: string; action?: unknown; inputKey?: string } = {},
): void {
  if (import.meta.env.VITE_BANDO_NATIVE_AUDIT !== "true" || typeof window === "undefined") return;
  const trace = (window as Window & {
    __bandoNativeCalls?: Array<{
      command: string; domain?: string; operation?: string; action?: unknown; inputKey?: string;
    }>;
  }).__bandoNativeCalls;
  if (Array.isArray(trace)) trace.push({ command, ...meta });
}

/** CI-only input signature. Preview event IDs/timestamps are excluded because
 * they are not meaningful read-only workflow inputs. Release excludes it. */
function auditNativeInputKey(operation: string, payload: unknown): string | undefined {
  if (import.meta.env.VITE_BANDO_NATIVE_AUDIT !== "true") return;
  try {
    const input = payload as { state?: unknown; draft?: unknown } | null;
    return JSON.stringify(operation === "reconcile_workflow" && input
      ? { state: input.state, draft: input.draft } : payload);
  } catch { return "<unserializable>"; }
}
/** Shared Tauri IPC entry point; domain operations remain checked by Rust. */
export function businessCoreCall<T>(
  domain: "recognition" | "rules" | "output" | "catalog_clients",
  operation: string,
  payload: unknown,
): Promise<T> {
  // Run the native command only once for equal, short-lived, pure read
  // projections. Every caller still receives the Rust result; no TS business
  // decision, response fallback, or mutation command is cached.
  return coalescePureNativeRead(domain, operation, payload, () => {
    auditNativeCall("business_core_call", {
      domain, operation, inputKey: auditNativeInputKey(operation, payload),
      action: payload && typeof payload === "object" && "action" in payload
        ? (payload as { action?: unknown }).action : undefined,
    });
    const started = performance.now();
    auditStateTransition("Rust IPC","request",{operation});
    return invoke<T>("business_core_call", { domain, operation, payload }).then(
      (value) => {
        auditStateTransition("Rust IPC","resolved",{operation,durationMs:Math.round(performance.now()-started)});
        return value;
      },
      (error) => {
        auditStateTransition("Rust IPC","rejected",{operation,durationMs:Math.round(performance.now()-started)});
        throw error;
      },
    );
  });
}

import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Order, Api, State, CatalogReport, OcrCapture, OfficialLink, ImportPreview } from "./types";
import productImages from "./tauri-product-images.json";



const notReady = (feature: string): never => {
  throw new Error(`${feature} 기능은 아직 구현되지 않았습니다.`);
};
const appWindow = () => getCurrentWindow();
const safeImage = (siteId?: string) => {
  if (!siteId || !Object.prototype.hasOwnProperty.call(productImages, siteId)) return null;
  const filename = (productImages as Record<string, string>)[siteId];
  return { data: `./product-images/${filename}`, source: "반도스탠드 수집 이미지" };
};

const api: Api = {
  saveProductWorkbook: (bytes) => invoke<string | null>("save_product_workbook", { bytes }),
  saveProductSyncWorkbook: (bytes) => invoke<string | null>("save_product_sync_workbook", { bytes }),
  saveFactoryWorkbook: (bytes) => invoke<string | null>("save_factory_workbook", { bytes }),
  saveStoreWorkbook: (bytes) => invoke<string | null>("save_store_workbook", { bytes }),
  saveSettlementWorkbook: (bytes) => invoke<string | null>("save_settlement_workbook", { bytes }),
  saveOrderHistoryWorkbook: (bytes) => invoke<string | null>("save_order_history_workbook", { bytes }),
  saveClientWorkbook: (bytes) => invoke<string | null>("save_client_workbook", { bytes }),
  saveRegistrationWorkbook: (kind, bytes) => invoke<string | null>("save_registration_workbook", { kind, bytes }),
  notificationList: () => invoke("hub_notifications_list"),
  notificationAdd: (notice) => invoke("hub_notifications_add", { notice }),
  notificationMarkRead: (id) => invoke("hub_notifications_mark_read", { id }),
  notificationDelete: (id) => invoke("hub_notifications_delete", { id }),
  notificationClear: () => invoke("hub_notifications_clear"),
  load: () => {
    auditStateTransition("Rust IPC","state-load-request");
    return invoke<State>("state_load").then((value) => {
      auditStateTransition("Rust IPC","state-load-resolved",{revision:value.revision??0});
      return value;
    });
  },
  save: (state) => {
    auditNativeCall("state_save");
    return invoke<State>("state_save", { state });
  },
  workingReceptionsLoad: () => invoke<Order[]>("working_receptions_load"),
  workingReceptionsReplace: (orders) =>
    invoke<void>("working_receptions_replace", { orders }),
  patchSettings: (changes, expected) => invoke<State>("settings_patch", { changes, expected }),
  setPrice: (productId, variantId, tier, price, expected) =>
    invoke<State>("price_set", { productId, variantId, tier, price, expected }),
  copy: (text) => {
    auditNativeCall("clipboard_write");
    return invoke<void>("clipboard_write", { text });
  },
  readClipboard: () => {
    auditNativeCall("clipboard_read");
    return invoke<string>("clipboard_read");
  },
  captureOcr: () => invoke<OcrCapture>("capture_ocr"),
  clipboardOcr: () => invoke<OcrCapture>("clipboard_ocr"),
  chooseOrderImageInboxDirectory: async () => (await invoke<string | null>("choose_order_image_inbox_directory")) ?? undefined,
  openOrderImageInboxDirectory: () => invoke<void>("open_order_image_inbox_directory"),
  orderImageWatchScan: () => invoke<string[]>("order_image_watch_scan"),
  orderImageOcr: (filename) => invoke<OcrCapture>("order_image_ocr", { filename }),
  cancelOcr: () => invoke<void>("cancel_ocr"),
  openProduct: (url) => invoke<void>("product_open", { target: url }),
  catalogReport: () => invoke<CatalogReport>("catalog_report"),
  backup: () => invoke<string>("backup_create"),
  backups: () => invoke<string[]>("backup_list"),
  restore: (filename) => invoke<void>("backup_restore", { filename }),
  productImage: async (id) => {
    const s = await invoke<State>("state_load");
    const product = s.products.find((p) => p.id === id);
    return safeImage(product?.siteId ?? product?.sourceProductId);
  },
  windowControl: async (action) => {
    if (action === "minimize") await appWindow().minimize();
    else if (action === "maximize") await appWindow().toggleMaximize();
    else if (action === "close") await appWindow().close();
  },
  requestClose: async (hasUnsavedChanges) => {
    if (hasUnsavedChanges && !window.confirm("저장하지 않은 변경사항이 있습니다. 종료할까요?")) return;
    await appWindow().close();
  },
  dockStatus: () => invoke("dock_status_native"),
  dockCycle: () => invoke("dock_cycle_native"),
  dockMode: (mode) => invoke("dock_mode_native", { mode }),
  dockStart: (handle, ratio) => invoke("dock_start_native", { handle, ratio }),
  dockStop: () => invoke("dock_stop_native"),
  dockSelect: (handle) => invoke("dock_select_native", { handle }),
  dockWindows: () => invoke("dock_windows_native"),
  dockRegion: (region) => invoke("dock_region_native", { region }),
  startupStatus: () => invoke("startup_status"),
  startupMetrics: () => invoke("startup_metrics"),
  retryStartup: () => invoke("retry_startup"),
  receiptArchive: () => invoke<{ id: string; orderId: string; record: Record<string, unknown> }[]>("receipt_archive"),
  // Confirmed receipts and corrections are written atomically by state_save.
  // The old Electron secondary-write IPC is intentionally not duplicated.
  chooseExcelDirectory: async () => (await invoke<string | null>("choose_excel_directory")) ?? undefined,
  openExcelDirectory: () => invoke<void>("open_excel_directory"),
  openDailyExcelDirectory: (kind) => invoke<void>("open_daily_excel_directory", { kind }),
  openExportFile: (id) => invoke<void>("open_export_file", { id }),
  openExportDirectory: (id) => invoke<void>("open_export_directory", { id }),
  previewImport: async (url) => {
    const html = await invoke<string>("fetch_official_product_html", { target: url });
    const state = await invoke<State>("state_load");
    return businessCoreCall<ImportPreview>("catalog_clients", "preview_official_product_html", {
      state, url, html, now: new Date().toISOString(),
      productId: crypto.randomUUID(), variantId: crypto.randomUUID(),
    });
  },
  // Suggests links from the bundled historical catalog; individual previews
  // still fetch their official product detail pages live.
  officialList: async () => {
    const links = await businessCoreCall<OfficialLink[]>("catalog_clients", "official_links", {
      report: await invoke<CatalogReport>("catalog_report"),
    });
    // Presentation-only sorting: business URL validation and deduplication
    // are authoritative in Rust, matching the original Korean UI ordering.
    return links.sort((a,b) => a.name.localeCompare(b.name,"ko"));
  },
  mobileStart: async () => notReady("스마트폰 연동"),
  mobileStop: async () => notReady("스마트폰 연동"),
  mobileStatus: async () => ({ running: false, urls: [] }),
  inbox: async () => [],
  archiveFactoryReply: (id, bytes) => invoke<string>("archive_factory_reply", { id, bytes }),
  chooseFactoryReplyDirectory: async () =>
    (await invoke<string | null>("choose_factory_reply_directory")) ?? undefined,
  factoryReplyWatchScan: () => invoke("factory_reply_watch_scan"),
  factoryReplyWatchCommit: (filename, id, candidate, expectedRevision) =>
    invoke("factory_reply_watch_commit", { filename, id, candidate, expectedRevision }),
  openFactoryFile: (id) => invoke<void>("open_factory_reply_file", { id }),
  approveFactoryConflict: (id, row, expectedRevision) =>
    invoke<void>("approve_factory_conflict", { id, row, expectedRevision }),
};

if (typeof window !== "undefined") window.bando = api;
