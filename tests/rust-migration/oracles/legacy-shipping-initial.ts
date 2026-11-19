/** Archived TypeScript comparison oracle. Never imported by the operational app. */
/** Fallback configuration only for very old/missing shipping records.
 * All active recommendation, group mutation and freight decisions are Rust-owned.
 */
import type { ShippingRule, ShippingState } from "../../../src/types.js";
const id = () => crypto.randomUUID();
const rule = (
  fees: Record<string, number | null>,
  notes = "",
  maxBundle: number | null = null,
  maxConfirmed = false,
  perItem: number | null = null,
): ShippingRule => ({
  version: id(),
  at: new Date().toISOString(),
  fees,
  maxBundle,
  maxConfirmed,
  perItem,
  notes,
});
export function initialShipping(): ShippingState {
  return {
    epoch: 1,
    productRules: {},
    productHistory: {},
    groups: [
      {
        id: "music",
        name: "일반 보면대",
        current: rule(
          { "1": 3000, "2": 4000, "3": null, "4": null, "5": null, "6": null },
          "최대 합포장 4개는 잠정값 / 직원 확인 필요",
          4,
          false,
        ),
        history: [],
      },
      {
        id: "microphone",
        name: "일반 마이크스탠드",
        current: rule(
          { "1": 3000, "2": 4000, "3": null, "4": null, "5": null, "6": null },
          "3~6개 운임 및 합포장 제한 확인 필요",
        ),
        history: [],
      },
      {
        id: "940",
        name: "940 계열",
        current: rule(
          { "1": null, "2": null, "3": null, "4": null, "5": null, "6": null },
          "1개 4,000원 및 합포장 제한 미확정 / 자동 추천하지 않음",
        ),
        history: [],
      },
      {
        id: "conductor",
        name: "지휘자 스탠드",
        current: rule({}, "확인된 기준: 개당 5,000원", 1, true, 5000),
        history: [],
      },
    ],
    combinations: [],
  };
}
