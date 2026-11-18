import { createFactoryWorkbook } from "./factory-export.js";

export interface StoreExportRow {
  queueId: string;
  shipmentId: string;
  orderId: string;
  values: string[];
}
export interface StoreExportPreview {
  revision: number;
  ids: string[];
  rows: StoreExportRow[];
}

/** Serialize the already-approved Rust STORE export preview. */
export async function createStoreWorkbook(preview: StoreExportPreview): Promise<number[]> {
  // The same Logen template is used; the STORE-only preview determines the rows.
  return createFactoryWorkbook(preview);
}
