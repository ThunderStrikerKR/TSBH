import type { WorkflowShipment } from "./workflow-types.js";
import type { Recipient } from "./types.js";
import type { Origin, Price } from "./types.js";
export const actualFreightOf = (ship: WorkflowShipment, row: Recipient, origin: Origin): Price =>
  Object.hasOwn(ship.actualFreight ?? {}, origin)
    ? (ship.actualFreight?.[origin] ?? null)
    : (row.actualFreight ?? null);
