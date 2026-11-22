import type ExcelJS from "exceljs";
import type { Client, State } from "./types";

export const CLIENT_HEADERS = [
  "__거래처ID", "거래처명", "주소", "우편번호",
  "전화번호", "휴대폰", "담당자", "연락처 메모", "가격등급", "결제방식",
  "운임 청구 제외", "부가세 기본 적용", "거래 상태",
  "거래처 별칭", "검색 식별자", "주 사용 계좌 ID", "보관 상태",
] as const;
// Read-only compatibility for previously exported V1 files. New exports use
// the 17-column V2 layout, with client.name as the only sender name.
const LEGACY_CLIENT_HEADERS = [
  "__거래처ID", "거래처명", "송장 표시 상호", "주소", "우편번호",
  "전화번호", "휴대폰", "담당자", "연락처 메모", "가격등급", "결제방식",
  "운임 청구 제외", "부가세 기본 적용", "거래 상태",
  "거래처 별칭", "검색 식별자", "주 사용 계좌 ID", "보관 상태",
] as const;
function migrateLegacyAlias(name: string, legacy: string, tags: string, identifiers: string): string {
  const compact = (v: string) => v.replace(/\s+/g, "").toLocaleLowerCase();
  const seen = new Set([name, ...tags.split(","), ...identifiers.split(",")]
    .map((v) => compact(v.trim())));
  const extra = legacy.split(",").map((v) => v.trim()).filter((v) => {
    const key = compact(v);
    if (!key || seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  return [tags.trim(), ...extra].filter(Boolean).join(", ");
}
export type ClientRowStatus = "ADD" | "UPDATE" | "UNCHANGED" | "HOLD" | "ERROR";
export interface ClientWorkbookRow {
  row: number;
  id: string;
  name: string;
  status: ClientRowStatus;
  changes: string[];
  errors: string[];
  client?: Client;
}
export interface ClientWorkbookPreview {
  revision: number;
  rows: ClientWorkbookRow[];
  missing: number;
}
const MAX_BYTES = 20 * 1024 * 1024;
const MAX_ROWS = 20000;
const tierText: Record<Client["tier"], string> = {
  wholesale: "도매", semi: "반도매", retail: "소매", special: "특수",
};
const paymentText = { PREPAID: "선입금", POSTPAID: "후불" } as const;

function workbookText(cell: ExcelJS.Cell): string {
  if (cell.isMerged || cell.type === 6 /* Formula */ ||
    (typeof cell.value === "object" && cell.value !== null && "formula" in cell.value)) {
    throw Error("수식이나 병합된 셀은 사용할 수 없습니다.");
  }
  const value = cell.value;
  if (value === null || value === undefined) return "";
  if (typeof value === "object") {
    if ("text" in value) return String(value.text ?? "").trim();
    if ("richText" in value) return value.richText.map((part) => part.text).join("").trim();
    throw Error("Excel 셀은 문자나 숫자로 입력하세요.");
  }
  return String(value).trim();
}
const rowValues = (client: Client): string[] => [
  client.id, client.name, client.address ?? "", client.postcode ?? "",
  client.phone ?? "", client.mobile ?? "", client.person ?? "", client.contact,
  tierText[client.tier], paymentText[client.payment],
  client.shippingFeeExcluded ? "예" : "아니오",
  client.defaultApplyVat === undefined ? "" : client.defaultApplyVat ? "예" : "아니오",
  client.active ? "거래중" : "거래중지",
  client.shortNames ?? "", client.identifiers, client.preferredAccountId ?? "",
  client.deletedAt ? "보관" : "사용",
];

export async function exportClientWorkbook(state: State): Promise<number[]> {
  const Excel = (await import("exceljs")).default;
  const workbook = new Excel.Workbook();
  const sheet = workbook.addWorksheet("거래처 목록");
  sheet.addRow([...CLIENT_HEADERS]);
  for (const client of state.clients) sheet.addRow(rowValues(client));
  sheet.views = [{ state: "frozen", ySplit: 1 }];
  sheet.autoFilter = { from: "A1", to: "Q1" };
  for (const [index, width] of
    [38, 30, 44, 14, 19, 19, 19, 24, 16, 16, 16, 18, 16, 32, 32, 39, 14].entries()) {
    // getColumn retains cells already inserted by addRow; resetting sheet.columns
    // at this point would silently delete all worksheet rows.
    const column = sheet.getColumn(index + 1);
    column.width = width;
    column.numFmt = "@";
  }
  sheet.getRow(1).font = { bold: true, color: { argb: "FFFFFFFF" } };
  sheet.getRow(1).fill = { type: "pattern", pattern: "solid", fgColor: { argb: "FF303030" } };
  const guide = workbook.addWorksheet("작성 안내");
  [
    ["구분", "사용 기준"],
    ["기본 작업", "현재 거래처 전체를 내려받아 수정한 뒤 업로드합니다. 누락된 행은 삭제하지 않습니다."],
    ["기존 거래처 수정", "__거래처ID를 변경하지 마세요. ID가 일치하는 행만 기존 거래처를 수정합니다."],
    ["신규 거래처 추가", "__거래처ID를 비우고 새 행을 작성합니다. 동일 상호나 전화번호가 있으면 검토 대상입니다."],
    ["공란", "기존 거래처의 공란은 기존 값을 유지합니다. 새 거래처의 필수 항목은 반드시 입력하세요."],
    ["값 지우기", "선택 항목을 지울 때만 [비우기]를 입력합니다."],
    ["상태 변경", "거래 상태는 거래중 또는 거래중지. Excel에서 행을 지워도 거래처를 삭제하지 않습니다."],
    ["가격 및 결제", "가격등급: 도매, 반도매, 소매, 특수 / 결제방식: 선입금, 후불"],
    ["설정 여부", "운임 청구 제외, 부가세 기본 적용은 예 또는 아니오로 입력합니다."],
    ["주 사용 계좌", "계좌 ID는 입금 계좌 안내 시트에서 복사합니다. 없는 ID는 저장할 수 없습니다."],
    ["보관 상태", "보관된 거래처의 정보를 Excel로 수정하거나 재활성화하지 않습니다."],
    ["특수 단가 및 변경 이력", "특수 단가 시트는 참고용입니다. 기존 품목별 특수 가격과 변경 이력은 업로드 시 유지합니다."],
    ["오류와 중복", "중복 ID, 이름/전화번호 충돌, 형식 오류가 있으면 변경 승인 전체를 막습니다."],
    ["안전장치", "업로드 미리보기 검토 및 확인을 마친 후 DB를 백업하고 기존 데이터에 변경사항만 반영합니다."],
    ["Excel 수식", "데이터 셀의 수식과 병합은 허용하지 않습니다."],
  ].forEach((values) => guide.addRow(values));
  guide.getColumn(1).width = 28;
  guide.getColumn(2).width = 108;
  const accounts = workbook.addWorksheet("입금 계좌 안내");
  accounts.addRow(["계좌 ID", "은행", "계좌주"]);
  for (const account of state.settings.accounts) {
    accounts.addRow([account.id, account.bank, account.holder]);
  }
  accounts.columns = [{ width: 40 }, { width: 22 }, { width: 28 }];
  const exceptions = workbook.addWorksheet("특수 단가 참고");
  exceptions.addRow(["거래처 ID", "거래처명", "상품 옵션 ID", "특수 단가 (원)"]);
  for (const client of state.clients) {
    for (const [variantId, amount] of Object.entries(client.overrides ?? {})) {
      exceptions.addRow([client.id, client.name, variantId, amount]);
    }
  }
  exceptions.columns = [{ width: 40 }, { width: 34 }, { width: 42 }, { width: 18 }];
  const meta = workbook.addWorksheet("__반도허브_관리정보");
  meta.addRow(["종류", "BANDO-HUB-CLIENTS-V2"]);
  meta.addRow(["DB버전", state.revision ?? 0]);
  meta.state = "veryHidden";
  const buffer = await workbook.xlsx.writeBuffer();
  return Array.from(new Uint8Array(buffer as ArrayBuffer));
}

export async function readClientWorkbookRows(state: State, file: File): Promise<{
  sourceRevision: number;
  cells: { row: number; cells: string[] }[];
}> {
  if (!/\.xlsx$/i.test(file.name) || file.size > MAX_BYTES) {
    throw Error("20MB 이하의 .xlsx 거래처 파일을 선택하세요.");
  }
  const Excel = (await import("exceljs")).default;
  const workbook = new Excel.Workbook();
  const bytes = new Uint8Array(await file.arrayBuffer());
  await workbook.xlsx.load(bytes as unknown as Parameters<typeof workbook.xlsx.load>[0]);
  const data = workbook.getWorksheet("거래처 목록");
  const meta = workbook.getWorksheet("__반도허브_관리정보");
  const version = meta ? workbookText(meta.getCell("B1")) : "";
  const legacy = version === "BANDO-HUB-CLIENTS-V1";
  const headers = legacy ? LEGACY_CLIENT_HEADERS : CLIENT_HEADERS;
  if (!data || !meta || (!legacy && version !== "BANDO-HUB-CLIENTS-V2")) {
    throw Error("반도허브에서 내려받은 거래처 전체 목록 Excel만 업로드할 수 있습니다.");
  }
  const sourceRevision = Number(workbookText(meta.getCell("B2")));
  if (!Number.isSafeInteger(sourceRevision) || sourceRevision < 0) throw Error("원본 DB 버전이 올바르지 않습니다.");
  for (const [index, header] of headers.entries()) {
    if (workbookText(data.getRow(1).getCell(index + 1)) !== header) {
      throw Error("거래처 Excel 열 이름이나 순서가 달라졌습니다: " + header);
    }
  }
  if (data.rowCount > MAX_ROWS + 1) throw Error("거래처 Excel은 최대 20,000행까지만 업로드할 수 있습니다.");
  const cells: { row: number; cells: string[] }[] = [];
  for (let i = 2; i <= data.rowCount; i++) {
    const values: string[] = [];
    try {
      for (let col = 1; col <= headers.length; col++) {
        const cell = data.getRow(i).getCell(col);
        if ((legacy ? [1, 5, 6, 7, 17] : [1, 4, 5, 6, 16]).includes(col)
            && typeof cell.value === "number") {
          throw Error(headers[col - 1] + "은 숫자가 아닌 텍스트 형식으로 입력하세요. 앞자리 0이 사라질 수 있습니다.");
        }
        values.push(workbookText(cell));
      }
      if (legacy) {
        const [id, name, displayName, ...rest] = values;
        const canonical = [id, name, ...rest];
        canonical[13] = migrateLegacyAlias(name, displayName, canonical[13] ?? "",
          canonical[14] ?? "");
        cells.push({ row: i, cells: canonical });
      } else {
        cells.push({ row: i, cells: values });
      }
    } catch (error) {
      // An unsafe row must block the entire import, never get silently skipped.
      throw Error(i + "행: " + String(error instanceof Error ? error.message : error));
    }
  }
  return { sourceRevision, cells };
}

/** Original TypeScript preview oracle retained for Rust parity comparisons. */
