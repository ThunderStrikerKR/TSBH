import type { State } from "./types";
import type { WorkflowSelection } from "./WorkflowBoard";
export function activeShipments(state: State) {
  return (
    state.workflow?.shipments.filter(
      (s) =>
        !s.transferred &&
        !s.legacyArchived &&
        state.orders.some((o) => o.id === s.orderId && !o.deletedAt) &&
        state.workflow?.groups.some((g) => g.id === s.groupId && !g.deletedAt),
    ) ?? []
  );
}
export function validSelection(state: State, selection?: WorkflowSelection) {
  const ss = activeShipments(state);
  const ship =
    ss.find(
      (s) =>
        s.groupId === selection?.groupId &&
        s.orderId === selection?.orderId &&
        (!selection.shipmentId || s.id === selection.shipmentId),
    ) ?? ss[0];
  return ship ? { orderId: ship.orderId, groupId: ship.groupId, shipmentId: ship.id } : undefined;
}

export function onlyDeletedGroups(state: State, orderId: string) {
  const groups =
    state.workflow?.groups.filter((g) =>
      state.workflow?.shipments.some((s) => s.orderId === orderId && s.groupId === g.id),
    ) ?? [];
  return groups.length > 0 && groups.every((g) => !!g.deletedAt);
}

/** Accordion visibility is controlled only by explicit selection and
 * title-button toggles. The absolute in-card hover overlay does not reflow cards. */
export function workflowGroupOpen(
  groupId: string,
  selectedGroupId?: string,
  manuallyClosedGroupId?: string,
): boolean {
  return selectedGroupId === groupId && manuallyClosedGroupId !== groupId;
}

/** A temporary preview row takes priority over the committed selection.
 * Hovering changes only what source/details show, never the actual selection. */
export function hoveredWorkflowSelection(
  state: State,
  groupId: string | undefined,
  selected?: WorkflowSelection,
  hoveredShipmentId?: string,
): WorkflowSelection | undefined {
  if (!groupId) return undefined;
  const shipments = activeShipments(state).filter((ship) => ship.groupId === groupId);
  const ship = shipments.find((s) => s.id === hoveredShipmentId)
    ?? shipments.find((s) => s.id === selected?.shipmentId)
    ?? shipments[0];
  return ship
    ? { orderId: ship.orderId, groupId: ship.groupId, shipmentId: ship.id }
    : undefined;
}
