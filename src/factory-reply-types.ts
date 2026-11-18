/** Types for native Rust factory reply projections and XLSX boundary.
 * No TypeScript business reducer is imported at runtime.
 */
export interface ReplyRow {
  row: number;
  name: string;
  address: string;
  contacts: string[];
  tracking: string;
  actual: number | null;
  issue?: string;
  shipmentId?: string;
  orderId?: string;
  applied?: boolean;
}
export interface FactoryReply {
  resolves?: { id: string; row: number };
  actor?: string;
  id: string;
  filename: string;
  archivedPath?: string;
  businessDate: string;
  at: string;
  rows: ReplyRow[];
  status: "APPLIED" | "REVIEW" | "PARTIAL";
  error?: string;
}
export interface FactoryReplyPreview {
  revision: number;
  reply: FactoryReply;
  rows: ReplyRow[];
  applied: number;
  held: number;
  status: FactoryReply["status"];
}

