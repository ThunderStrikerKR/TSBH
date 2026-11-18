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

/** During a stage hover the hovered group temporarily owns the expanded
 * viewport. Once the hover clears, restore the exact committed accordion
 * state, including a manually closed selected group. */
export function workflowGroupOpen(
  groupId: string,
  selectedGroupId?: string,
  hoveredGroupId?: string,
  manuallyClosedGroupId?: string,
): boolean {
  return hoveredGroupId
    ? hoveredGroupId === groupId
    : selectedGroupId === groupId && manuallyClosedGroupId !== groupId;
}

/** Resolve the hovered action's own order without changing the committed
 * selection. For multi-recipient groups keep the currently selected recipient
 * if it belongs to that group; otherwise use its first active shipment. */
export function hoveredWorkflowSelection(
  state: State,
  groupId: string | undefined,
  selected?: WorkflowSelection,
): WorkflowSelection | undefined {
  if (!groupId) return undefined;
  const shipments = activeShipments(state).filter((ship) => ship.groupId === groupId);
  const ship = shipments.find((s) => s.id === selected?.shipmentId) ?? shipments[0];
  return ship
    ? { orderId: ship.orderId, groupId: ship.groupId, shipmentId: ship.id }
    : undefined;
}
