/** Instant Rust item price projection, independent of workflow reconciliation.
 * UI stores only the last value for an identical price-affecting input.
 * React never multiplies prices or computes billable quantities.
 */
import {useEffect,useMemo,useState} from "react";
import {businessCoreCall} from "../tauri-bridge";
import type {Order,Price} from "../types";

type NativeLines = {
  orderId: string;
  lineAmounts: Array<{recipientId:string;itemId:string;amount:Price}>;
};

export function useNativeOrderLineAmounts(draft:Order) {
  const fingerprint = useMemo(() => JSON.stringify({
    orderId: draft.id,
    rows: draft.recipients.map(row=>({
      id: row.id,
      items: row.items.map(item=>({
        id: item.id,productId:item.productId,variantId:item.variantId,
        unitPrice:item.unitPrice,quantity:item.quantity,
      })),
    })),
  }),[draft]);
  const [resolved,setResolved]=useState<{fingerprint:string;result:NativeLines}>();
  const [failure,setFailure]=useState<{fingerprint:string;reason:string}>();
  useEffect(()=>{
    let active=true;
    void businessCoreCall<NativeLines>("rules","preview_order_lines",{draft})
      .then(result=>{
        if (!active)return;
        setResolved({fingerprint,result});
        setFailure(undefined);
      }).catch(error=>{
        if (!active)return;
        setFailure({fingerprint,reason:String(error)});
      });
    return()=>{active=false};
    // fingerprint contains all price-affecting values; unrelated text edits
    // cannot cancel a settled Rust line price.
  },[fingerprint]);
  const ready=resolved?.fingerprint===fingerprint &&
    resolved.result.orderId===draft.id;
  return {
    ready,
    lineAmounts:ready?resolved!.result.lineAmounts:undefined,
    error:failure?.fingerprint===fingerprint?failure.reason:"",
  };
}
