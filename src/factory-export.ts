
export const LOGEN_HEADERS = [
  "수하인명", "수하인우편번호", "수하인주소", "수하인전화번호", "수하인핸드폰번호",
  "택배수량", "택배운임", "운임구분", "품목명", "", "메시지", "송하인명",
  "(송)우편", "(송)주소1", "(송)전화", "(송)핸드폰",
] as const;

export interface FactoryExportRow {
  queueId: string;
  shipmentId: string;
  orderId: string;
  values: string[];
}
export interface FactoryExportPreview {
  revision: number;
  ids: string[];
  rows: FactoryExportRow[];
}

/** ExcelJS serializer only. Export approval, rows and history are owned by Rust. */
export async function createFactoryWorkbook(preview: FactoryExportPreview): Promise<number[]> {
  const Excel = (await import("exceljs")).default;
  const book = new Excel.Workbook();
  const sheet = book.addWorksheet("로젠택배");
  sheet.addRow([...LOGEN_HEADERS]);
  for (const row of preview.rows) sheet.addRow(row.values);
  sheet.views = [{ state: "frozen", ySplit: 1 }];
  sheet.autoFilter = { from: "A1", to: "P1" };
  // Shipping identifiers (010/020), phone numbers and postcodes must retain zeros.
  for (const [i, width] of [16, 14, 48, 18, 18, 12, 12, 12, 64, 3, 35, 18, 12, 48, 18, 18].entries()) {
    const column = sheet.getColumn(i + 1);
    column.width = width;
    column.numFmt = "@";
  }
  sheet.getRow(1).font = { bold: true };
  return Array.from(new Uint8Array(await book.xlsx.writeBuffer() as ArrayBuffer));
}

