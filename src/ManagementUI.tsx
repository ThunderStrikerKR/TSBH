import { HelpTooltip } from "./ui/HelpTooltip";
import { useEffect, useState } from "react";
import { confirmAction, promptText } from "./dialogs";
import { useNativeClients } from "./native-catalog-queries";
import { businessCoreCall } from "./tauri-bridge";
import type { NoticeSink } from "./shared/notices";
import type { Client, State } from "./types";
import { Modal } from "./ui";
export const tierText = {
  wholesale: "도매",
  semi: "반도매",
  retail: "소매",
  special: "특수",
};
export function TierBadge({ client }: { client: Client }) {
  return <span className={"tag tier-" + client.tier}>{tierText[client.tier]}</span>;
}
export function ClientPicker({
  clients,
  selected,
  raw,
  onSelect,
  compact = false,
}: {
  clients: Client[];
  selected?: Client;
  raw: string;
  onSelect: (c: Client) => void;
  compact?: boolean;
}) {
  const [query, setQuery] = useState(""),
    [all, setAll] = useState(false),
    [popup, setPopup] = useState(false),
    [dismissed, setDismissed] = useState("");
  const nativeClients = useNativeClients(clients, query, raw);
  const results = query ? nativeClients.results : [];
  const suggested = nativeClients.suggested;
  const key = suggested.map((c) => c.id).sort().join("|");
  useEffect(() => {
    if (selected || !key) setPopup(false);
    if (!key) setDismissed("");
  }, [key, selected?.id]);
  const choose = (c: Client) => {
    onSelect(c);
    setQuery("");
    setAll(false);
    setPopup(false);
  };
  const row = (c: Client) => (
    <button className="client-result" key={c.id} onClick={() => choose(c)}>
      <span>
        <b>{c.name}</b>
        <small>
          {c.address || "주소 미등록"} / {c.phone || "전화 미등록"}
        </small>
      </span>
      <TierBadge client={c} />
    </button>
  );
  return (
    <div className="client-picker">
      {!compact && (
        <div className="form-row">
          <input
            aria-label="거래처 통합 검색"
            placeholder="거래처명 / 송장 상호 / 주소 / 전화번호 검색"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <button onClick={() => setAll(true)}>전체 거래처 목록</button>
        </div>
      )}
      {nativeClients.error && <p role="alert">{nativeClients.error}</p>}
      {query && (
        <div className="client-results">
          {results.length ? (
            results.slice(0, 20).map(row)
          ) : nativeClients.pending ? (
            <p className="muted">검색 중</p>
          ) : (
            <p className="muted">검색 결과가 없습니다.</p>
          )}
        </div>
      )}
      {selected && (
        <div className="selected-client">
          <span>
            <b>{selected.name}</b>
            <small>{selected.address || "거래처 주소 미등록"}</small>
          </span>
          <TierBadge client={selected} />
        </div>
      )}
      {!selected && suggested.length > 0 && !popup && (
        <button className="client-choice-reopen" onClick={() => setPopup(true)}>
          거래처 후보 {suggested.length}곳 확인
        </button>
      )}
      {popup && !selected && suggested.length > 0 && (
        <Modal
          title="거래처 후보 선택"
          onClose={() => {
            setPopup(false);
            setDismissed(key);
          }}
        >
          <p>원문에 일치하는 거래처가 {suggested.length}곳입니다. 적용할 거래처를 선택하세요.</p>
          {suggested.map(row)}
        </Modal>
      )}
      {all && (
        <Modal title="전체 거래처 목록" onClose={() => setAll(false)}>
          <div className="client-chooser-list">{clients.filter((c) => !c.deletedAt).map(row)}</div>
        </Modal>
      )}
    </div>
  );
}
const backupLabel = (path: string) => {
  const name = path.split(/[\\/]/).pop() ?? path;
  if (name.startsWith("monthly-")) return "월간 최초 자동 백업 (영구 보관)";
  if (name.startsWith("auto-")) return "일반 자동 백업 (30일 보관)";
  if (name.startsWith("pre-restore-")) return "복원 직전 안전 백업 (영구 보관)";
  if (name.startsWith("manual-")) return "수동 백업 (영구 보관)";
  return "기존 백업 (자동 삭제 안 함)";
};
export function BackupPanel({
  onRefresh,
  onError,
}: {
  onRefresh: () => Promise<void>;
  onError: NoticeSink;
}) {
  const [files, setFiles] = useState<string[]>([]);
  return (
    <section className="card settings">
      <h2 className="settings-help-heading">데이터 백업 및 복구 <HelpTooltip title="데이터 백업 및 복구" purpose="컴퓨터에 저장된 주문, 상품, 거래처 등의 업무 정보를 안전하게 보관하고 필요하면 이전 상태로 되돌립니다." when="중요한 설정을 바꾸기 전이나 데이터 복구가 필요할 때 사용합니다." steps={["지금 백업을 눌러 현재 자료의 사본을 만듭니다.", "백업 목록에서 보관된 파일을 확인합니다.", "이전 상태로 돌아가야 한다면 해당 파일의 이 백업으로 복구 버튼을 누르고 확인합니다."]} result="선택한 백업 시점의 업무 정보를 복구할 수 있습니다." caution="자동 백업은 프로그램을 시작할 때 확인합니다. 일반 자동 백업은 최근 30일간 보관하고, 각 달에 처음 생성된 백업과 수동 백업은 계속 보관합니다. 복구하면 주문과 작업 기록도 해당 시점으로 돌아갑니다." /></h2>

      <div className="actions">
        <button
          onClick={async () => {
            try {
              onError("백업 완료: " + (await window.bando!.backup()), "success");
              setFiles(await window.bando!.backups());
            } catch (e) {
              onError(String(e), "error");
            }
          }}
        >
          지금 백업
        </button>
        <button
          onClick={async () => {
            try {
              setFiles(await window.bando!.backups());
            } catch (e) {
              onError(String(e), "error");
            }
          }}
        >
          백업 목록
        </button>
      </div>
      {files.map((f) => (
        <div className="history" key={f}>
          <span>{f.split(/[\\/]/).pop()} <small>{backupLabel(f)}</small></span>
          <button
            onClick={async () => {
              if (
                !(await confirmAction("선택한 시점의 전체 데이터베이스로 복원할까요? 현재 데이터는 먼저 안전 백업으로 보관합니다. 주문과 감사 기록도 선택한 시점으로 되돌아갑니다."))
              )
                return;
              try {
                await window.bando!.restore(f);
                await onRefresh();
                onError(
                  "백업을 복구했습니다. 기존 편집 화면 대신 주문 내역을 다시 열어 주세요.",
                  "success",
                );
              } catch (e) {
                onError(String(e), "error");
              }
            }}
          >
            이 백업으로 복구
          </button>
        </div>
      ))}
    </section>
  );
}
export function CategoriesPanel({
  state,
  onSave,
  onError,
}: {
  state: State;
  onSave: (s: State) => Promise<void>;
  onError: NoticeSink;
}) {
  const [name, setName] = useState(""),
    [parent, setParent] = useState("");
  const cats = state.categories ?? [];
  return (
    <section className="card">
      <h2>카테고리 / 하위 분류 관리</h2>
      <div className="form-row category-tools">
        <input
          aria-label="새 카테고리명"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="카테고리명"
        />
        <select
          aria-label="상위 카테고리"
          value={parent}
          onChange={(e) => setParent(e.target.value)}
        >
          <option value="">최상위 분류</option>
          {cats
            .filter((c) => !c.parentId)
            .map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
        </select>
        <button
          onClick={async () => {
            if (!name.trim()) return;
            if (
              !/[가-힣]/.test(name) ||
              name.trim() === "미분류" ||
              (!parent && name.trim() === "악세사리")
            ) {
              onError(
                "한글 분류명을 사용하세요. 미분류는 검토 상태이며, 스탠드 악세사리는 별도 대분류를 사용합니다.",
              );
              return;
            }
            if (cats.some((c) => c.name === name.trim() && c.parentId === (parent || null))) {
              onError("같은 분류가 있습니다.");
              return;
            }
            try {
              const { state: next } = await businessCoreCall<{ state: State }>(
                "catalog_clients", "legacy_add_category", {
                  state, categoryId: crypto.randomUUID(),
                  name, parentId: parent || null,
                },
              );
              await onSave(next);
              setName("");
            } catch (e) {
              onError(String(e), "error");
            }
          }}
        >
          분류 추가
        </button>
      </div>
      <details>
        <summary>등록된 분류 {cats.length}개 / 이름 수정</summary>
        {cats.map((c) => (
          <div className="form-row category-row" key={c.id}>
            <span>
              {c.parentId ? "↳ " : ""}
              {c.name}
            </span>
            <button
              onClick={async () => {
                const value = await promptText("새 분류명", c.name);
                if (!value?.trim()) return;
                try {
                  const { state: next } = await businessCoreCall<{ state: State }>(
                    "catalog_clients", "legacy_rename_category", {
                      state, categoryId: c.id, name: value,
                    },
                  );
                  await onSave(next);
                } catch (e) {
                  onError(String(e), "error");
                }
              }}
            >
              이름 수정
            </button>
          </div>
        ))}
      </details>
    </section>
  );
}
