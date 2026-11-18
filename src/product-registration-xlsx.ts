import { categoryPath } from "./category-tree";
import { catalogFromCategory, withCatalogRoots } from "./category-roots";
import type { RegistrationBatch, RegistrationRow } from "./product-registry";
import type { State } from "./types";

/** A row is one actual selling configuration. Existing prices are never overwritten. */
export const REGISTRATION_HEADERS = [
  "최종 등록명", "브랜드", "색상", "구성 구분", "규격",
  "판매 단위", "상품군", "카테고리 경로", "도매가", "반도매가",
  "소매가", "상품 링크", "비고",
] as const;
/** Previous column order is accepted only as a separate legacy format. */
export interface RegistrationWorkbookPreview {
  fingerprint: string;
  revision: number;
  rows: RegistrationRow[];
}

export async function exportRegistrationTemplate(state: State): Promise<number[]> {
  const excelJs = (await import("exceljs")).default;
  const wb = new excelJs.Workbook();
  const sheet = wb.addWorksheet("상품 등록");
  sheet.addRow([...REGISTRATION_HEADERS]);
  sheet.views = [{ state: "frozen", ySplit: 1 }];
  sheet.autoFilter = { from: "A1", to: "M1" };
  sheet.getRow(1).font = { bold: true, color: { argb: "FFFFFFFF" } };
  sheet.getRow(1).fill = { type: "pattern", pattern: "solid", fgColor: { argb: "FF303030" } };
  const widths = [34, 20, 15, 19, 21, 13, 17, 55, 14, 14, 14, 48, 36];
  sheet.columns.forEach((column, i) => {
    column.width = widths[i];
    if (i >= 8 && i <= 10) column.numFmt = "#,##0";
  });
  for (let row = 2; row <= 500; row++) {
    sheet.getCell(row, 7).dataValidation = {
      type: "list", allowBlank: false, formulae: ['"반도스탠드,반도몰"'],
      showErrorMessage: true, error: "반도스탠드 또는 반도몰을 선택하세요.",
    };
  }
  const guide = wb.addWorksheet("작성 안내");
  [
    ["항목", "작성 기준"],
    ["작성 단위", "실제 판매 가능한 완성 상품(모델, 색상, 규격, 포함 구성) 하나당 한 행. 구분 요소는 최종 등록명에도 기재"],
    ["상품군", "반도스탠드 또는 반도몰 중 하나를 정확히 선택"],
    ["카테고리 경로", "카테고리 경로 목록 시트의 최하위 전체 경로를 그대로 입력"],
    ["도매가 / 반도매가 / 소매가", "공란 또는 0은 미등록. 기존 가격은 덮어쓰지 않음"],
    ["중복 또는 미확인 항목", "미리보기에서 중복 또는 보류로 표시하며 자동 등록하지 않음"],
    ["상품 링크", "반도스탠드 또는 반도몰 공식 상품 상세페이지 URL만 허용"],
    ["수식 및 병합 셀", "허용하지 않음"],
  ].forEach((row) => guide.addRow(row));
  guide.getColumn(1).width = 38;
  guide.getColumn(2).width = 85;
  const paths = wb.addWorksheet("카테고리 경로 목록");
  paths.addRow(["상품군", "카테고리 경로"]);
  const all = withCatalogRoots(state.categories);
  for (const c of all) {
    const catalog = catalogFromCategory(all, c.id);
    if (!catalog || all.some((child) => child.parentId === c.id) || c.active === false) continue;
    paths.addRow([
      catalog === "BANDOSTAND" ? "반도스탠드" : "반도몰",
      categoryPath(all, c.id).map((item) => item.name).join(" / "),
    ]);
  }
  paths.getColumn(1).width = 20;
  paths.getColumn(2).width = 70;
  const output = await wb.xlsx.writeBuffer();
  return Array.from(new Uint8Array(output as ArrayBuffer));
}
export async function exportRegistrationResults(batch: RegistrationBatch): Promise<number[]> {
  const excelJs = (await import("exceljs")).default;
  const wb = new excelJs.Workbook();
  const sheet = wb.addWorksheet("등록 결과");
  sheet.addRow(["행", ...REGISTRATION_HEADERS, "결과", "사유", "등록 상품 ID"]);
  let created = 0;
  for (const item of batch.rows) {
    const id = item.status === "READY" ? batch.createdIds[created++] ?? "" : "";
    sheet.addRow([item.row, ...REGISTRATION_HEADERS.map((h) => item.values[h] ?? ""),
      { READY: "등록 완료", ERROR: "오류", HOLD: "보류", DUPLICATE: "중복" }[item.status],
      item.errors.join(" / ") || item.duplicateIds.join(", "), id]);
  }
  sheet.views = [{ state: "frozen", ySplit: 1 }];
  sheet.columns.forEach((column, i) => { column.width = i === 0 ? 9 : i === 14 ? 40 : 23; });
  const output = await wb.xlsx.writeBuffer();
  return Array.from(new Uint8Array(output as ArrayBuffer));
}
