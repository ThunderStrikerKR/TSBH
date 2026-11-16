/** Projection freshness, not TypeScript eligibility or issue classification.
 * Include every input consumed by Rust issues::order_issues / action flag.
 * Exclude raw text, shipping totals, VAT, draft metadata and unrelated UI
 * fields so unrelated native workflow or pricing replies cannot re-trigger
 * staff-issue loading after the correct Rust result is already visible.
 */
import type {Order} from "../types";

export function nativeOrderIssuesKey(order:Order):string {
  return JSON.stringify({
    id:order.id,
    clientId:order.clientId,
    orderType:order.orderType,
    receptionVersion:order.receptionVersion,
    clientMatch:order.clientMatch,
    issueResolutions:order.issueResolutions,
    recipients:order.recipients.map(r=>({
      id:r.id,name:r.name,address:r.address,addressStatus:r.addressStatus,
      phone:r.phone,mobile:r.mobile,unassignedEvidence:r.unassignedEvidence,
      candidates:r.candidates,confirmedFields:r.confirmedFields,sources:r.sources,
      items:r.items.map(i=>({
        id:i.id,name:i.name,productId:i.productId,variantId:i.variantId,
        quantity:i.quantity,quantityConfirmed:i.quantityConfirmed,
        quantityApproval:i.quantityApproval,unitPrice:i.unitPrice,
        match:i.match,issues:i.issues,color:i.color,
        modelProposal:i.modelProposal,sources:i.sources,
      })),
    })),
  });
}
