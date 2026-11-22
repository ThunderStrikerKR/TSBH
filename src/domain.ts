import { companyParty } from "./order-types.js";
import { z } from "zod";
import { aliasProblems } from "./matching.js";
import type { Alias, Client, Item, Order, Product, Recipient, Settings, State } from "./types.js";
export const unassignedClient: Client = {
  id: "",
  name: "",
  contact: "",
  identifiers: "",
  active: true,
  tier: "wholesale",
  shippingFeeExcluded: false,
  payment: "PREPAID",
  overrides: {},
};
export const uid = () => crypto.randomUUID();
export const normalizeCode = (s: string) => s.toUpperCase().replace(/\s/g, "");
const colorMap: Record<string, string> = {
  gold: "Gold",
  골드: "Gold",
  "rose gold": "Rose Gold",
  로즈골드: "Rose Gold",
  "cosmo black": "Cosmo Black",
  cosmoblack: "Cosmo Black",
  코스모블랙: "Cosmo Black",
  chrome: "Chrome",
  크롬: "Chrome",
  black: "Black",
  블랙: "Black",
  검정: "Black",
  검정색: "Black",
  gray: "Gray",
  grey: "Gray",
  그레이: "Gray",
  회색: "Gray",
  white: "White",
  화이트: "White",
  흰색: "White",
  silver: "Silver",
  실버: "Silver",
  red: "Red",
  빨강: "Red",
  blue: "Blue",
  파랑: "Blue",
  brown: "Brown",
  브라운: "Brown",
  clear: "Clear",
  클리어: "Clear",
  frost: "Frost",
  프로스트: "Frost",
};
export const normalizeColor = (s: string) => colorMap[s.trim().toLowerCase()] ?? s.trim();
export const money = z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER);
export const priceSchema = money.nullable();
const variantSchema = z.object({
  id: z.string().min(1),
  color: z.string().min(1),
  wholesale: priceSchema,
  semi: priceSchema,
  retail: priceSchema,
  url: z.string(),
  source: z.enum(["DEMO", "MANUAL", "OFFICIAL"]),
  collectedAt: z.string().optional(),
});
export const productSchema = z.object({
  id: z.string().min(1),
  code: z.string().min(1),
  name: z.string().min(1),
  category: z.string(),
  origin: z.enum(["STORE", "FACTORY"]),
  defaultColor: z.string().min(1),
  active: z.boolean(),
  // Each business product has exactly one price record; website configurations are separate products.
  variants: z.array(variantSchema).length(1),
  createdAt: z.string(),
  updatedAt: z.string(),
});
export function validateProduct(p: Product, products: Product[]) {
  productSchema.parse(p);
  if (
    products.some(
      (x) =>
        x.id !== p.id &&
        !products.some((existing) => existing.id === p.id) &&
        x.active !== false && !x.deletedAt && !x.supersededBy &&
        x.catalog === p.catalog &&
        x.name.normalize("NFKC").toLocaleLowerCase().replace(/\s+/g,"") ===
          p.name.normalize("NFKC").toLocaleLowerCase().replace(/\s+/g,""),
    )
  )
    throw Error("같은 상품명이 이미 있습니다. 판매 구성을 구별할 수 있게 이름을 수정하세요.");
  if (!p.variants.some((v) => normalizeColor(v.color) === normalizeColor(p.defaultColor)))
    throw Error("상품 색상이 가격 기록과 일치하지 않습니다.");
  for (const v of p.variants)
    if (v.url && !validProductUrl(v.url))
      throw Error("상품 링크는 확인된 반도스탠드 / 반도몰 상세페이지 URL을 입력하세요.");
}
export function validProductUrl(value: string) {
  try {
    const u = new URL(value);
    return (
      u.protocol === "https:" &&
      ["bandostand.com", "www.bandostand.com", "bandomall.co.kr", "www.bandomall.co.kr"].includes(
        u.hostname,
      ) &&
      u.pathname.startsWith("/product/")
    );
  } catch {
    return false;
  }
}
export function validateAlias(a: Alias, all: Alias[], products: Product[]) {
  const problems = aliasProblems(a, all, products);
  if (problems.length) throw Error(problems.join("\n"));
  if (
    all.some(
      (x) =>
        x.id !== a.id &&
        x.clientId === a.clientId &&
        normalizeCode(x.text) === normalizeCode(a.text),
    )
  )
    throw Error("같은 적용 범위에 이미 등록된 별칭입니다.");
}
export function resolvePrice(p: Product, variantId: string, client: Client) {
  if (!client.id) return null;
  const v = p.variants.find((v) => v.id === variantId);
  const value =
    client.tier === "special"
      ? (client.overrides?.[variantId] ??
        v?.legacyIds?.map((id) => client.overrides?.[id]).find((p) => p !== undefined) ??
        v?.wholesale ??
        null)
      : (v?.[client.tier] ?? null);
  return value === 0 ? null : value;
}
export function optionCodes(text: string) {
  const explicit = [...text.matchAll(/\bBD\s*(\d[A-Za-z0-9-]*)/gi)].map((m) => "BD " + m[1]);
  const numeric = text.match(/^(\d{3}[A-Za-z0-9-]*)(?![A-Za-z0-9-])/);
  return explicit.length ? explicit : numeric ? ["BD " + numeric[1]] : [];
}
export function itemFromProduct(
  p: Product,
  client: Client,
  color?: string,
  quantity: number | null = 1,
  selectedVariantId?: string,
  // Optional authoritative native quote: bypass TS pricing in migrated UI.
  priceOverride?: number | null,
): Item {
  const explicit = selectedVariantId
    ? p.variants.find((variant) => variant.id === selectedVariantId)
    : undefined;
  const c = normalizeColor(explicit?.color ?? color ?? p.defaultColor);
  const eligible = p.variants.filter((variant) => normalizeColor(variant.color) === c);
  // Color alone does not distinguish same-color options such as a bag
  // included/excluded, wheels included/excluded or perforated top.
  // Even an explicitly supplied price ID cannot override unresolved source
  // configurations. New orders require one validated independent sale product.
  const selectable = p.variants.length === 1 && !p.requiresOption;
  const v = !selectable ? undefined : selectedVariantId ? explicit : eligible[0];
  // Preserve the curated business-facing product label for every order line.
  // Distinct products can share a model code (e.g. instrument/fixture variants).
  const basicName = (p.name || p.code).replace(/^반도스탠드\s*/i, "").trim();
  const detail = p.variants.length > 1 ? (v?.option ?? "").trim() : "";
  const productName = [basicName, ...(detail ? [detail] : [])].join(" / ");
  const configurationIncomplete = !!v && ["가방", "바퀴"].some((kind) => {
    const pattern = new RegExp(kind + "\\s*(?:미포함|포함|미장착|장착|없음|있음)");
    return p.variants.some((variant) => pattern.test(variant.option ?? "")) &&
      !pattern.test(v.option ?? "");
  });
  return {
    id: uid(),
    productId: p.id,
    variantId: v?.id ?? null,
    raw: p.name,
    name: p.variants.some((variant) => variant.source === "DEMO") &&
      !productName.startsWith("[가상] ")
      ? "[가상] " + productName : productName,
    color: c,
    quantity,
    unitPrice: v
      ? priceOverride === undefined ? resolvePrice(p, v.id, client) : priceOverride
      : null,
    origin: p.origin,
    defaulted: !color,
    issues: v
      ? (p.colorStatus === "MISSING" ||
          (!p.saleUnit && ["색상 미표기", "", undefined].includes(p.defaultColor))) &&
        !color
        ? ["기본 색상 확인 필요"]
        : (configurationIncomplete ? ["가방 또는 바퀴 포함 / 미포함 정보 확인 필요"] : [])
      : p.requiresOption
        ? ["상품 구성 미확정 / 판매 구성을 확인하세요"]
        : ["등록된 독립 상품을 다시 선택하세요"],
    url: v?.url ?? "",
    catalog: p.catalog,
  };
}
export function orderClient(o: Order, state: State) {
  if (o.orderType === "RETAIL") return o.clientSnapshot ?? companyParty(state.settings);
  return (o.confirmedAt || o.pricesLocked) && o.clientSnapshot
    ? o.clientSnapshot
    : (state.clients.find((c) => c.id === o.clientId) ?? o.clientSnapshot);
}
export function clientCandidates(text: string, clients: Client[]) {
  const query = text.toLowerCase().replace(/\s/g, "");
  if (!query) return [];
  return clients.filter(
    (c) =>
      !c.deletedAt &&
      [c.name, c.phone, c.mobile, c.address, c.shortNames, c.identifiers].some(
        (v) => v && v.toLowerCase().replace(/\s/g, "").includes(query),
      ),
  );
}
export function suggestClient(raw: string, clients: Client[]) {
  return clients.filter(
    (c) =>
      !c.deletedAt &&
      [c.name, ...(c.shortNames ?? "").split(","), ...(c.identifiers ?? "").split(",")]
          .some((n) => n.trim().length >= 2 && raw.includes(n.trim())),
  );
}
export function recommendAccount(o: Order, s: Settings, products: Product[] = [], client?: Client) {
  if (o.accountMode === "MANUAL") return s.accounts.find((a) => a.id === o.accountId);
  if (o.orderType !== "RETAIL") {
    const preferred = client?.preferredAccountId ?? o.clientSnapshot?.preferredAccountId;
    const account = s.accounts.find((a) => a.id === preferred);
    if (account) return account;
  }
  const items = o.recipients.flatMap((r) => r.items);
  const onlyMall =
    items.length > 0 &&
    items.every(
      (i) => (i.catalog ?? products.find((p) => p.id === i.productId)?.catalog) === "BANDOMALL",
    );
  const id = onlyMall
    ? s.brandAccounts?.BANDOMALL
    : s.brandAccounts?.BANDOSTAND || s.defaultAccountId;
  return s.accounts.find((a) => a.id === id);
}
export function roundVat(base: number) {
  return Math.round(base * 0.1);
}
export function totals(o: Order, c: Client) {
  c = o.pricesLocked && o.clientSnapshot ? o.clientSnapshot : c;
  const items = o.recipients.flatMap((r) => r.items);
  const valid =
    items.length > 0 &&
    items.every(
      (i) =>
        i.productId &&
        i.variantId &&
        money.safeParse(i.unitPrice).success &&
        Number.isInteger(i.quantity) &&
        (i.quantity ?? 0) > 0,
    );
  const candidate = valid ? items.reduce((s, i) => s + i.unitPrice! * i.quantity!, 0) : null;
  const subtotal = candidate !== null && Number.isSafeInteger(candidate) ? candidate : null;
  const shipping = c.shippingFeeExcluded || o.freightCode === "020" ? 0 : o.shippingFee;
  const complete = subtotal !== null && money.safeParse(shipping).success;
  const vat =
    o.pricesLocked && o.vatAmount !== null
      ? o.vatAmount
      : !o.applyVat
        ? 0
        : complete
          ? roundVat(subtotal! + shipping!)
          : null;
  const total = complete ? subtotal! + shipping! + vat! : null;
  if (total !== null && !Number.isSafeInteger(total))
    return { subtotal: null, shipping, vat: null, total: null };
  return { subtotal, shipping, vat, total };
}
export const won = (n: number | null) =>
  n === null ? "확인 필요" : n.toLocaleString("ko-KR") + "원";
export const statusNames = {
  RECEIVED: "주문 접수",
  REVIEW: "정보 확인 필요",
  STATEMENT: "명세서 작성 완료",
  NOTIFIED: "안내 완료",
  WAITING: "입금 대기",
  APPROVED: "입금 확인 / 후불 승인",
  QUEUED: "공장 전달 대기",
  EXPORTED: "공장 전달 완료",
};

