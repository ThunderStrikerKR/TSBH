import type ExcelJS from "exceljs";
import { registeredProducts, saleOption, sortProducts } from "./product-models";
import type { State } from "./types";

// The hidden identifiers intentionally match the legacy Electron price workbook.
// Never use row order, an editable name, or a model code as the sole record key.
export const PRICE_HEADERS = [
  "상품명", "상품설명", "상품 색상", "도매가", "반도매가", "소매가",
  "__상품ID", "__가격기록ID", "__외부구성원본", "__색상ID",
] as const;
const LEGACY_PRICE_HEADERS = [...PRICE_HEADERS.slice(0,8),
  "__모델코드", ...PRICE_HEADERS.slice(8)] as const;

export async function exportPriceWorkbook(state: State): Promise<number[]> {
  const excelJs = (await import("exceljs")).default;
  const wb = new excelJs.Workbook();
  const sheet = wb.addWorksheet("상품 가격표");
  sheet.addRow([...PRICE_HEADERS]);
  for (const p of sortProducts(registeredProducts(state.products))) {
    for (const v of p.variants) {
      sheet.addRow([
        p.name, p.subtitle ?? "", saleOption(p, v), v.wholesale, v.semi, v.retail,
        p.id, v.id, v.option ?? "", v.color,
      ]);
    }
  }
  sheet.views = [{ state: "frozen", ySplit: 1 }];
  sheet.autoFilter = { from: "A1", to: "J1" };
  sheet.getRow(1).font = { bold: true, color: { argb: "FFFFFFFF" } };
  sheet.getRow(1).fill = { type: "pattern", pattern: "solid", fgColor: { argb: "FF303030" } };
  const widths = [27, 36, 28, 17, 17, 17, 23, 23, 24, 20];
  sheet.columns.forEach((column, i) => {
    column.width = widths[i];
    column.hidden = i >= 6;
    if (i >= 3 && i <= 5) column.numFmt = "#,##0";
  });
  for (let row = 2; row <= sheet.rowCount; row++) {
    for (let col = 4; col <= 6; col++) {
      const letter = String.fromCharCode(64 + col);
      sheet.getCell(row, col).dataValidation = {
        type: "custom",
        allowBlank: true,
        formulae: [`OR(ISBLANK(${letter}${row}),AND(ISNUMBER(${letter}${row}),${letter}${row}>=0,MOD(${letter}${row},1)=0),${letter}${row}="삭제")`],
        showErrorMessage: true,
        errorTitle: "가격 확인",
        error: "0 이상의 원 단위 정수 또는 삭제를 입력하세요.",
      };
    }
  }
  const buf = await wb.xlsx.writeBuffer();
  return Array.from(new Uint8Array(buf as ArrayBuffer));
}

function text(value: ExcelJS.CellValue): string {
  if (value === null || value === undefined) return "";
  if (typeof value === "object") return "text" in value ? String(value.text).trim() : "";
  return String(value).trim();
}

/** XLSX parsing only. All price, SKU and history decisions belong to Rust. */
export interface PriceWorkbookSource {
  rows: { row: number; values: string[]; unsafe: boolean }[];
}
export async function readPriceWorkbookRows(file: File): Promise<PriceWorkbookSource> {
  if (file.size > 20 * 1024 * 1024) throw Error("20MB 이하의 가격표 파일을 선택하세요.");
  if (!/\.xlsx$/i.test(file.name)) throw Error(".xlsx 파일만 사용할 수 있습니다.");
  const Excel = (await import("exceljs")).default;
  const wb = new Excel.Workbook();
  await wb.xlsx.load(new Uint8Array(await file.arrayBuffer()) as unknown as Parameters<typeof wb.xlsx.load>[0]);
  const sheet = wb.worksheets[0];
  if (!sheet) throw Error("엑셀 시트가 없습니다.");
  const matches=(headers:readonly string[]) =>
    headers.every((header,index)=>text(sheet.getRow(1).getCell(index+1).value)===header) &&
    !text(sheet.getRow(1).getCell(headers.length+1).value);
  const headers=matches(PRICE_HEADERS)?PRICE_HEADERS:
    matches(LEGACY_PRICE_HEADERS)?LEGACY_PRICE_HEADERS:undefined;
  if (!headers) throw Error("반도허브에서 내려받은 가격표 양식과 열이 다릅니다.");
  if (sheet.rowCount > 20001) throw Error("한 번에 최대 2만 행까지 검증할 수 있습니다.");
  const rows: PriceWorkbookSource["rows"] = [];
  for (let rowIndex = 2; rowIndex <= sheet.rowCount; rowIndex++) {
    const cells = headers.map((_, i) => sheet.getRow(rowIndex).getCell(i + 1));
    const rawValues = cells.map((cell) => text(cell.value));
    const values = headers === LEGACY_PRICE_HEADERS
      ? [...rawValues.slice(0,8),...rawValues.slice(9)] : rawValues;
    if (!values.some(Boolean)) continue;
    const unsafe = cells.some(cell =>
      cell.isMerged || (cell.value && typeof cell.value === "object" && "formula" in cell.value));
    rows.push({ row: rowIndex, values, unsafe: Boolean(unsafe) });
  }
  return { rows };
}
