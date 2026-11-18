/** Price sheet cells are read by ExcelJS but all pricing decisions are Rust-owned. */
import { readPriceWorkbookRows } from "./product-price-xlsx";
import { registeredProducts, saleOption, variantLabel } from "./product-models";
import { businessCoreCall } from "./tauri-bridge";
import type { BulkPreview, State } from "./types";

export async function previewNativePriceWorkbook(state: State, file: File): Promise<BulkPreview> {
  const source = await readPriceWorkbookRows(file);
  // Presentation labels are generated the same way as the downloaded XLSX.
  // Rust independently matches immutable product and variant IDs.
  const labels = registeredProducts(state.products).flatMap(product =>
    product.variants.map(variant => ({
      productId: product.id, variantId: variant.id,
      display: saleOption(product, variant), fallback: variantLabel(variant),
    })));
  return businessCoreCall<BulkPreview>("catalog_clients", "preview_price_workbook_rows", {
    state, source, labels, at: new Date().toISOString(),
  });
}
