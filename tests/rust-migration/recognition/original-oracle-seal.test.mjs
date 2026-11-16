import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";

// Fix the TypeScript reference independently from the Rust implementation.
const shipping = readFileSync(new URL("./original-shipping-20261116.ts", import.meta.url), "utf8")
  .replace('from "../../../src/types.js";', 'from "./types.js";');
const raw = Buffer.from(shipping, "utf8");
const blobSha = createHash("sha1").update(`blob ${raw.length}\0`).update(raw).digest("hex");
const price = readFileSync(new URL("./original-price-20261116.ts", import.meta.url), "utf8");
const frozenPrice = price.slice(price.indexOf("export function resolvePrice")).trim();
const currentDomain = readFileSync(new URL("../../../src/domain.ts", import.meta.url), "utf8");
const currentPrice = currentDomain.slice(currentDomain.indexOf("export function resolvePrice"),
  currentDomain.indexOf("const escape", currentDomain.indexOf("export function resolvePrice"))).trim();
const mergeOracle = readFileSync(new URL("./ts-reception-merge-oracle.ts", import.meta.url), "utf8");

test("shipping oracle is exact original pre-migration Git blob", () => {
  assert.equal(blobSha, "ee60df8101acf71e298fd289aabd593e85c84b21");
});
test("frozen original shipping keeps non-packable limit", () => {
  assert.match(shipping, /r\.packable === false && q > 1/);
});
test("current pure price resolver matches frozen pre-migration body", () => {
  assert.equal(currentPrice, frozenPrice);
});
test("merge differential oracle never mirrors Rust's temporary null price/fee", () => {
  assert.match(mergeOracle, /item\.unitPrice = resolvePrice\(/);
  assert.match(mergeOracle, /next\.shippingFee = pricingClient\.shippingFeeExcluded \? 0 : reco\.total/);
  assert.doesNotMatch(mergeOracle, /item\.unitPrice = null/);
  assert.doesNotMatch(mergeOracle, /next\.shippingVersion = ""/);
});
