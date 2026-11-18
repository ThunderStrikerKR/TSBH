/** Shared workbook data contracts. Real price, catalog and row decisions are Rust-owned. */
import type { Product } from "./types";

export type ProductRowStatus = "ADD" | "UPDATE" | "UNCHANGED" | "HOLD" | "ERROR";
export interface ProductSyncRow {
  sheet: "상품 목록" | "상품별 가격";
  row: number;
  id: string;
  name: string;
  status: ProductRowStatus;
  changes: string[];
  errors: string[];
}
export interface ProductSyncPreview {
  revision: number;
  products: Product[];
  rows: ProductSyncRow[];
  missingProducts: number;
  missingOptions: number;
  createdProducts: number;
  updatedProducts: number;
  createdOptions: number;
  updatedOptions: number;
}
export interface WorkbookSource {
  revision: number;
  products: { row: number; cells: string[] }[];
  options: { row: number; cells: string[] }[];
}
