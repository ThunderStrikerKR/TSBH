import { navigation, type PageId } from "./navigation";
export function AppHeader({
  page,
  onNavigate,
  onWindow,
  onNotifications,
  notificationCount = 0,
  notificationError = false,
}: {
  page: PageId;
  onNavigate: (page: PageId) => void;
  onNotifications?: () => void;
  notificationCount?: number;
  notificationError?: boolean;
  onWindow: (action: "minimize" | "maximize" | "close") => void;
}) {
  return (
    <header
      data-tauri-drag-region
      className="app-header"
      onDoubleClick={(event) => {
        if (event.target === event.currentTarget) onWindow("maximize");
      }}
    >
      <button className="wordmark hub-brand" onClick={() => onNavigate("접수")}>
        <img src="./branding/bando-symbol.svg" alt="반도 로고" />
        <span className="hub-brand-title">BANDO HUB</span>
      </button>
      <nav data-tauri-drag-region aria-label="업무 메뉴">
        {navigation.map((item) => (
          <button
            className={page === item.id ? "current" : ""}
            aria-current={page === item.id ? "page" : undefined}
            key={item.id}
            onClick={() => onNavigate(item.id)}
          >
            {item.label}
          </button>
        ))}
      </nav>
      <div className="header-actions">
        {onNotifications && (
          <button className="header-notifications-button" onClick={onNotifications}
            title={notificationError ? "알림 데이터베이스 확인 필요" : "알림"}
            aria-label={notificationError ? "알림 저장 오류, 확인 필요" :
              notificationCount ? `알림 ${notificationCount}건` : "알림"}>
            <svg viewBox="0 0 24 24" aria-hidden="true" width="16" height="16"
              fill="none" stroke="currentColor" strokeWidth="1.8"
              strokeLinecap="round" strokeLinejoin="round">
              <path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9" />
              <path d="M10 21h4" />
            </svg>
            <span>알림</span>
            {notificationError
              ? <small className="header-notifications-count" aria-hidden="true">!</small>
              : notificationCount > 0 && (
                <small className="header-notifications-count" aria-hidden="true">
                  {notificationCount > 99 ? "99+" : notificationCount}
                </small>
              )}
          </button>
        )}
      </div>
      <div className="window-controls">
        {(["minimize", "maximize", "close"] as const).map((action, index) => (
          <button
            key={action}
            aria-label={["창 최소화", "창 최대화 또는 복원", "프로그램 닫기"][index]}
            onClick={() => onWindow(action)}
          >
            {index === 0 ? (
              <svg viewBox="0 0 12 12" aria-hidden="true">
                <path d="M1 8.5h10" />
              </svg>
            ) : index === 1 ? (
              <svg viewBox="0 0 12 12" aria-hidden="true">
                <rect x="1.5" y="1.5" width="9" height="9" />
              </svg>
            ) : (
              <svg viewBox="0 0 12 12" aria-hidden="true">
                <path d="m1.5 1.5 9 9m0-9-9 9" />
              </svg>
            )}
          </button>
        ))}
      </div>
    </header>
  );
}
