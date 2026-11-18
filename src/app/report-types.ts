/** Native order-history and settlement response DTOs only.
 * TypeScript does not calculate business amounts or statuses.
 */
import type { Client, Order, Recipient } from "../types";

export type OrderHistoryStatus =
  | "처리 중" | "엑셀 추출 대기" | "엑셀 추출 완료" | "공장 회신 완료";
export interface HistoryRow {
  order: Order;
  client: Client | undefined;
  recipients: Recipient[];
  total: number | null;
  billing: { subtotal: number | null; shipping: number | null; vat: number | null; total: number | null };
  shipmentIds: string[];
  deleted: boolean;
  status: OrderHistoryStatus;
}

export interface SettlementOrderRow {
  orderId: string;
  clientName: string;
  date: string;
  status: string;
  paymentTerms: "선입금" | "후불" | "소매" | "미확인";
  subtotal: number | null;
  shippingCharge: number | null;
  vat: number | null;
  total: number | null;
  paidAllocated: number;
  paymentApproved: number;
  outstanding: number | null;
  freightCost: number | null;
  freightComplete: boolean;
  cashEvidenceMissing: boolean;
  legacyDate: boolean;
  shipmentIds: string[];
}
export interface SettlementPaymentRow {
  id: string;
  paidAt: string;
  dealer: string;
  account: string;
  amount: number;
  allocated: number;
  unallocated: number;
  memo: string;
}
export interface DailySettlementReport {
  date: string;
  revision: number;
  orderRows: SettlementOrderRow[];
  paymentRows: SettlementPaymentRow[];
  billedKnown: number;
  billedUnknown: number;
  productKnown: number;
  shippingKnown: number;
  vatKnown: number;
  paidCash: number;
  allocatedCash: number;
  unallocatedCash: number;
  postpaidKnown: number;
  outstandingKnown: number;
  outstandingUnknown: number;
  actualFreightKnown: number;
  freightUnknown: number;
  approvedPaymentKnown: number;
  warnings: string[];
}

