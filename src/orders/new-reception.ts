/** A blank in-memory React reception form only.
 * Business approval, persistence and workflow mutation remain native Rust.
 */
import { blankOrder } from "../seed.js";
import type { Order, OrderType } from "../types.js";
export function newReception(clientId = "", orderType: OrderType = "AGENCY"): Order {
  return { ...blankOrder(clientId), orderType, receptionVersion: 1 };
}
