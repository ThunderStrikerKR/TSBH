import type { Order } from "../types";

type Waiter = { resolve: () => void; reject: (reason: unknown) => void };
type PendingBatch = { orders: Order[]; waiters: Waiter[] };

/**
 * Serialized and coalesced SQLite working-copy writes. Every enqueue waits
 * until the snapshot it requested (or a newer snapshot) actually commits.
 * In particular, no caller can receive a completed older promise while its
 * newer edit has only been queued and not yet written.
 */
export class WorkingReceptionAutosave {
  private pending: PendingBatch | undefined;
  private active = false;

  constructor(private readonly save: (orders: Order[]) => Promise<void>) {}

  enqueue(orders: Order[]): Promise<void> {
    const snapshot = structuredClone(orders);
    return new Promise<void>((resolve, reject) => {
      if (this.pending) {
        // Coalesced callers all wait for the most recent snapshot. This is
        // safe because that snapshot contains the entire working order set.
        this.pending.orders = snapshot;
        this.pending.waiters.push({ resolve, reject });
      } else {
        this.pending = { orders: snapshot, waiters: [{ resolve, reject }] };
      }
      if (!this.active) {
        this.active = true;
        void this.drain();
      }
    });
  }

  private async drain(): Promise<void> {
    while (this.pending) {
      const batch = this.pending;
      this.pending = undefined;
      try {
        await this.save(batch.orders);
        for (const waiter of batch.waiters) waiter.resolve();
      } catch (error) {
        // A failed transaction must be reported to EVERY caller waiting for
        // a pending write. Retain the most recent snapshot for the next retry,
        // but never retry indefinitely while the DB is unavailable.
        // Another enqueue may run while await save(...) is suspended.
        // TS control-flow analysis otherwise narrows this field to undefined.
        const newer = this.pending as PendingBatch | undefined;
        this.pending = {
          orders: newer?.orders ?? batch.orders,
          waiters: [],
        };
        for (const waiter of batch.waiters) waiter.reject(error);
        if (newer) for (const waiter of newer.waiters) waiter.reject(error);
        this.active = false;
        return;
      }
    }
    // No asynchronous gap exists between the final pending check and this
    // reset, so another enqueue cannot be stranded behind a settled promise.
    this.active = false;
  }
}
