import type { Order } from "../types";

/** Serialize/coalesce saves so an older native completion cannot replace
 * newer staff input. This layer persists only: never performs business rules.
 */
export class WorkingReceptionAutosave {
  private wanted: Order[] | undefined;
  private inFlight: Promise<void> | undefined;
  constructor(private readonly save: (orders: Order[]) => Promise<void>) {}

  enqueue(orders: Order[]): Promise<void> {
    this.wanted = structuredClone(orders);
    if (!this.inFlight) {
      const current = this.drain();
      this.inFlight = current;
      void current.then(() => {
        if (this.inFlight === current) this.inFlight = undefined;
      }, () => {
        if (this.inFlight === current) this.inFlight = undefined;
      });
    }
    return this.inFlight;
  }

  private async drain(): Promise<void> {
    while (this.wanted !== undefined) {
      const next = this.wanted;
      this.wanted = undefined;
      try {
        await this.save(next);
      } catch (error) {
        if (this.wanted === undefined) this.wanted = next;
        throw error;
      }
    }
  }
}
