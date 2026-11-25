import type { Order } from "./types";
import { FactoryNotifications } from "./FactoryNotifications";
import { FactoryReplyWatcher } from "./FactoryReplyWatcher";
import { usePersistedNotifications } from "./app/usePersistedNotifications";
import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { AppHeader } from "./app/AppHeader";
import { StartupRecoveryPanel } from "./app/StartupRecoveryPanel";
import { DailySettlement } from "./app/DailySettlement";
import { OrderHistory } from "./app/OrderHistory";
import { useAppModel } from "./app/useAppModel";
import { HubShell } from "./HubShell";
import { defaultPanels } from "./HubPanels";
import { KakaoDockPicker } from "./KakaoDockPicker";
import { MemoHubWorkspace as HubWorkspace } from "./HubWorkspace";
import { useEventCallback } from "./shared/useEventCallback";
import { NoticeContext } from "./shared/NoticeContext";
import { EmptyState } from "./ui/PageHeading";
import { ScreenBoundary } from "./ui/ScreenBoundary";
const SettingsView = lazy(() =>
  import("./app/SettingsView").then((module) => ({
    default: module.SettingsView,
  })),
);
const ClientManager = lazy(() =>
  import("./ClientManager").then((module) => ({
    default: module.ClientManager,
  })),
);
const ProductManager = lazy(() =>
  import("./ProductManager").then((module) => ({
    default: module.ProductManager,
  })),
);
const ShippingManager = lazy(() =>
  import("./ShippingManager").then((module) => ({
    default: module.ShippingManager,
  })),
);
const WorkflowQueue = lazy(() =>
  import("./WorkflowQueue").then((module) => ({
    default: module.WorkflowQueue,
  })),
);
const noopDockReset = () => {};
export function App() {
  const [notifications, setNotifications] = useState(false);
  const lastCriticalNoticeId = useRef<number | undefined>(undefined);
  const model = useAppModel(),
    { state, draft, tab } = model;
  // Dock/notification repaints must not recreate handlers for live orders.
  const workspaceNew = useEventCallback(() => { void model.run(model.newOrder); });
  const workspaceSettings = useEventCallback(() => { void model.navigate("설정"); });
  const workspaceProducts = useEventCallback(() => { void model.navigate("상품"); });
  const workspaceSelect = useEventCallback(model.onSelectWorkflow);
  const workspaceWorkflowSave = useEventCallback(model.onWorkflowSave);
  const workspaceSave = useEventCallback(model.saveOrder);
  const workspaceCommit = useEventCallback(model.commit);
  const workspaceChange = useEventCallback((
    value: Order | ((previous: Order) => Order),
  ) => model.setDraft((previous) =>
    previous ? (typeof value === "function" ? value(previous) : value) : previous,
  ));
  // Wait until the Tauri startup registered the managed SQLite connection.
  const { inbox, hiddenCount, storageError, add: addNotification, markRead, remove, clear } =
    usePersistedNotifications(Boolean(state && draft));
  // Never turn routine saves, ordinary field validation or reply-file events
  // into persistent notifications. Only serious infrastructure failures that
  // were explicitly classified by the notice owner enter this inbox.
  useEffect(() => {
    const notice = model.notice;
    if (!state || !draft || !notice?.critical || lastCriticalNoticeId.current === notice.id) return;
    lastCriticalNoticeId.current = notice.id;
    void addNotification({
      priority: "critical",
      source: "시스템", tone: "error",
      title: "긴급 조치 필요", detail: notice.message,
    });
  }, [model.notice, state, draft, addNotification]);
  if (!state || !draft) {
    if (model.startup.phase === "ERROR" || model.startup.phase === "RETRYING") {
      return (
        <StartupRecoveryPanel
          startup={model.startup}
          retrying={model.startupBusy}
          onRetry={model.retryStartup}
        />
      );
    }
    return (
      <main className="loading" role="status">
        <h1>반도허브</h1>
        <p>업무 데이터베이스를 안전하게 확인하는 중입니다.</p>
      </main>
    );
  }
  return (
    <NoticeContext.Provider value={model.setNotice}>
      <div className="app-v03" aria-busy={model.busy}>
        <FactoryReplyWatcher
          enabled={Boolean(state.settings.factoryReplyDirectory)}
          paused={model.dirty || model.busy}
          onRefresh={model.refresh}
        />
        <AppHeader
          onNotifications={() => setNotifications(true)}
          notificationCount={inbox.filter((entry) => !entry.read).length}
          notificationError={Boolean(storageError)}
          page={tab}
          onNavigate={model.navigate}
          onWindow={(action) => void model.run(() => model.windowAction(action))}
        />
        {model.dockCandidates.length > 0 && (
          <KakaoDockPicker
            windows={model.dockCandidates}
            busy={model.dockBusy}
            onClose={() => model.setDockCandidates([])}
            onSelect={(handle) => void model.run(() => model.selectDock(handle))}
          />
        )}
        {notifications && (
          <FactoryNotifications
            notices={inbox}
            hiddenCount={hiddenCount}
            onRead={(id) => { void markRead(id); }}
            onDelete={(id) => { void remove(id); }}
            onClear={() => { void clear(); }}
            storageError={storageError}
            onClose={() => setNotifications(false)}
          />
        )}
        <HubShell
          ratios={state.settings.panelRatios ?? defaultPanels}
          dock={model.dock}
          busy={model.dockBusy}
          onConnect={() => void model.run(model.docking)}
          onDockMode={(mode) => void model.run(() => model.setDockMode(mode))}
          onDockStop={() => void model.run(model.stopDock)}
          onSave={(ratios) =>
            void model.run(() =>
              model.commit({
                ...state,
                settings: { ...state.settings, panelRatios: ratios },
              }),
            )
          }
          onReset={() =>
            void model.run(() =>
              model.commit({
                ...state,
                settings: {
                  ...state.settings,
                  panelRatios: defaultPanels,
                  receptionVerticalRatio: 0.5,
                },
              }),
            )
          }
        >
          <main
            className={
              tab === "접수"
                ? "desk-container"
                : "management-container" +
                  (["상품", "거래처"].includes(tab) ? " management-fixed" : "")
            }
          >
            <div className="hub-desk-host" hidden={tab !== "접수"}>
              <ScreenBoundary onRetry={model.refresh} onError={model.setNotice}>
                <HubWorkspace
                  key={model.restoreEpoch}
                  onNew={workspaceNew}
                  onSettings={workspaceSettings}
                  onRawEditDirty={model.setRawEditDirty}
                  onRememberSelection={model.setWorkflowSelection}
                  onRegisterOrderLeaveGuard={model.registerOrderLeaveGuard}
                  externalKakao={true}
                  onResetDock={noopDockReset}
                  workflowSelection={model.workflowSelection}
                  onSelectWorkflow={workspaceSelect}
                  onWorkflowSave={workspaceWorkflowSave}
                  state={state}
                  draft={draft}
                  onChange={workspaceChange}
                  onSave={workspaceSave}
                  onSaveState={workspaceCommit}
                  onNotice={model.setNotice}
                  onManageProducts={workspaceProducts}
                />
              </ScreenBoundary>
            </div>
            <Suspense fallback={<EmptyState title="화면 준비 중…" />}>
              <ScreenBoundary key={tab} onRetry={model.refresh} onError={model.setNotice}>
                {tab === "상품" && (
                  <ProductManager
                    state={state}
                    onSave={model.commit}
                    onRefresh={model.refresh}
                    onNotice={model.setNotice}
                  />
                )}{" "}
                {tab === "거래처" && (
                  <ClientManager
                    state={state}
                    onSave={model.commit}
                    onRefresh={model.refresh}
                    onNotice={model.setNotice}
                  />
                )}{" "}
                {tab === "운임" && (
                  <ShippingManager state={state} onSave={model.commit} onNotice={model.setNotice} />
                )}{" "}
                {tab === "주문내역" && (
                  <OrderHistory
                    state={state}
                    query={model.query}
                    onQuery={model.setQuery}
                    date={model.historyDate}
                    onDate={model.setHistoryDate}
                    onNotice={model.setNotice}
                  />
                )}{" "}
                {tab === "결산" && (
                  <DailySettlement state={state} date={model.settlementDate} onDate={model.setSettlementDate} onNotice={model.setNotice} />
                )}{" "}
                {tab === "공장목록" && (
                  <WorkflowQueue
                    onSettings={() => void model.navigate("설정")}
                    state={state}
                    onSave={model.commit}
                    onRefresh={model.refresh}
                    onOpen={(id) => {
                      const order = state.orders.find((order) => order.id === id);
                      if (order) void model.open(order);
                    }}
                    onNotice={model.setNotice}
                  />
                )}{" "}
                {tab === "설정" && (
                  <SettingsView
                    state={state}
                    onSaveSettings={model.saveSettings}
                    onNotice={model.setNotice}
                    onRestored={model.afterRestore}
                  />
                )}
              </ScreenBoundary>
            </Suspense>
          </main>
        </HubShell>
      </div>
    </NoticeContext.Provider>
  );
}
