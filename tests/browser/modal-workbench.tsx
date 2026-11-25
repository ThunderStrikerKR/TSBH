/** Browser smoke fixture only. Never imported by the application.
 * Mounts actual current components against synthetic IPC, not retired settings UI.
 */
import React, { useEffect, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Modal } from "../../src/ui";
import { NoticeContext } from "../../src/shared/NoticeContext";
import { CorrectionPanel } from "../../src/CorrectionPanel";
import { IssueResolver } from "../../src/IssueResolver";
import { ProductSearchDialog } from "../../src/ProductSearchDialog";
import { ProductCategoryPicker } from "../../src/ProductDetailComponents";
import { ManualClassification } from "../../src/orders/ManualClassification";
import { RetailOrderDialog } from "../../src/orders/RetailOrderDialog";
import { ProductManager } from "../../src/ProductManager";
import { ClientManager } from "../../src/ClientManager";
import { RegistrationManager } from "../../src/RegistrationManager";
import { FactoryReplyImport } from "../../src/FactoryReplyImport";
import { ReceiptArchive } from "../../src/ReceiptArchive";
import type { State, Order } from "../../src/types";
import type { NoticeSink } from "../../src/shared/notices";

const modes = ["NORMAL","EMPTY","ERROR","BUSY","LONG","WARNING","SUCCESS","FAILURE","CANCEL"];
const popups = ["common","correction","manual","resolver","product-search","category","retail",
  "product","client","registration","reply","receipt"];
const noWrite = async (): Promise<void> => { throw Error("CI modal fixture forbids business saves"); };
const notice: NoticeSink = (message, kind) => {
  if(kind === "error") console.warn("CI modal fixture:", message);
};
function Workbench({state}: {state: State}) {
  const [popup,setPopup] = useState("common");
  const [mode,setMode] = useState("NORMAL");
  const [open,setOpen] = useState(false);
  const [text,setText] = useState("");
  const close = () => setOpen(false);
  useEffect(() => {
    if(!open || popup !== "common" || mode !== "BUSY") return;
    const timer = window.setTimeout(() => setMode("NORMAL"),1800);
    return () => window.clearTimeout(timer);
  },[open,popup,mode]);
  const ship = state.workflow?.shipments.find(s=>s.id==="ci-store-ship");
  const order = state.orders.find(o=>o.id===ship?.orderId);
  if(!ship || !order) throw Error("Missing synthetic shipment for modal fixture");
  const recipient = order.recipients.find(r=>r.id===ship.recipientId);
  if(!recipient || !recipient.items.length) throw Error("Missing synthetic recipient/item");
  const draft: Order = {...structuredClone(order),id:"ci-modal-manual",confirmedAt:undefined,
    raw:"매장시험수취인\n서울 테스트구 확인로 3\n기타줄 2개",analysisRaw:undefined,
    recipients:[{...structuredClone(recipient),items:recipient.items.map(i=>({...i,raw:"기타줄 2개"}))}]};
  const preview = () => {
    if(popup==="common") return <Modal title={"UI 상태 시험 / "+mode}
      className="developer-preview-dialog" busy={mode==="BUSY"}
      draft={mode==="CANCEL"?{text}:undefined} onClose={close}>
      {mode==="EMPTY"?<p>표시할 데이터가 없습니다.</p>:<>
        {mode==="BUSY"&&<p role="status">시험용 처리 중입니다.</p>}
        {["ERROR","WARNING","FAILURE"].includes(mode)&&<p role="alert" className="form-error">시험용 오류 및 경고</p>}
        {mode==="SUCCESS"&&<p role="status">저장 성공</p>}
        <label htmlFor="ci-modal-input">검증용 입력</label>
        <input id="ci-modal-input" aria-invalid={mode==="ERROR"} value={text} onChange={e=>setText(e.target.value)}/>
        {mode==="LONG"&&<p className="developer-long-text">{"긴 입력 및 안내 문구 스크롤 검증. ".repeat(240)}</p>}
      </>}
      <div className="row-end"><button onClick={close}>취소</button>
        <button className="primary" disabled={mode==="ERROR"} onClick={close}>시험 확인</button></div>
    </Modal>;
    if(popup==="correction") return <CorrectionPanel state={state} sid={ship.id} onNotice={notice} onSave={noWrite} onClose={close}/>;
    if(popup==="manual") return <ManualClassification state={state} order={draft} onNotice={notice}
      onApply={()=>{throw Error("CI modal fixture forbids apply");}} onClose={close}/>;
    if(popup==="resolver") return <IssueResolver state={state} order={draft} recipient={draft.recipients[0]}
      item={draft.recipients[0].items[0]} onNotice={notice} onApply={noWrite} onClose={close}/>;
    if(popup==="product-search") return <ProductSearchDialog state={state} onChoose={close} onClose={close}/>;
    if(popup==="category") return <ProductCategoryPicker categories={state.categories??[]} onSelect={close} onClose={close}/>;
    if(popup==="retail") return <RetailOrderDialog state={state} onSave={noWrite} onClose={close}/>;
    if(popup==="client") return <ClientManager state={state} onNotice={notice} onSave={noWrite} onRefresh={async()=>{}}/>;
    if(popup==="product") return <ProductManager state={state} onNotice={notice} onSave={noWrite} onRefresh={async()=>{}}/>;
    if(popup==="registration") return <RegistrationManager state={state} onNotice={notice} onSave={noWrite} onRefresh={async()=>{}}/>;
    if(popup==="reply") return <FactoryReplyImport previewOnly onNotice={notice} onSave={noWrite} onRefresh={async()=>{}}/>;
    return <ReceiptArchive/>;
  };
  return <NoticeContext.Provider value={notice}><section className="card developer-modal-workbench">
    <label htmlFor="dev-popup">CI 검사 대상</label>
    <select id="dev-popup" value={popup} onChange={e=>{close();setPopup(e.target.value);}}>
      {popups.map(p=><option key={p} value={p}>{p}</option>)}
    </select>
    {popup==="common"&&<select id="dev-popup-state" value={mode} onChange={e=>{close();setMode(e.target.value);}}>
      {modes.map(m=><option key={m} value={m}>{m}</option>)}
    </select>}
    <button type="button" onClick={()=>{setText("");setOpen(v=>!v);}}>{open?"시험 화면 닫기":"선택한 실제 화면 열기"}</button>
    {open&&(["client","product","registration","reply","receipt"].includes(popup)
      ? <div className="developer-live-container">{preview()}</div>:preview())}
  </section></NoticeContext.Provider>;
}

let root: Root|undefined;
let host: HTMLElement|undefined;
export function unmountModalFixture() { root?.unmount();host?.remove();root=undefined;host=undefined; }
export function mountModalFixture(state: State) {
  unmountModalFixture();
  const panel = document.querySelector('[role="tabpanel"][aria-labelledby="settings-tab-system"]');
  if(!panel) throw Error("CI modal fixture needs mounted system panel");
  host=document.createElement("div");host.id="ci-modal-fixture";
  panel.appendChild(host);root=createRoot(host);
  root.render(<Workbench state={structuredClone(state)}/>);
}
