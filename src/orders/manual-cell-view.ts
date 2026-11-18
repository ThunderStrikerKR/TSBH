/** UI-only manual classification cell keys and value rendering.
 * Assignment validation and mutation belong exclusively to Rust.
 */
import type { Order, Recipient } from "../types.js";
export type Cell = { recipientId: string; field: string; itemId?: string };
export type Block = NonNullable<Order["manualClassification"]>["blocks"][number];
export type Assignment = NonNullable<Order["manualClassification"]>["assignments"][string];
export type ManualModel = {
  rows: Recipient[];
  blocks: Block[];
  assignments: Record<string, Assignment>;
};
export const cellKey = (cell: Cell) => `${cell.recipientId}:${cell.itemId ?? ""}:${cell.field}`;

export function manualValue(model: ManualModel, cell: Cell): string {
  const r = model.rows.find((r) => r.id === cell.recipientId);
  if (!r) return "";
  if (cell.itemId) {
    const i = r.items.find((i) => i.id === cell.itemId);
    return cell.field === "product" ? (i?.name ?? "") : String(i?.quantity ?? "");
  }
  return String(r[cell.field as "name" | "address" | "mobile" | "phone" | "note"] ?? "");
}
