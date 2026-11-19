/** UI-facing projection type. The legacy hub-messages engine lives only in test oracles. */
import type { ValidationIssue } from "./types";

export interface StaffBlock extends Partial<ValidationIssue> {
  choices?: { label: string; value: string }[];
  recipientId: string;
  title: string;
  detail: string;
  customer?: string;
  linkKey?: string;
  itemId?: string;
}
