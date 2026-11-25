# Round 102 — Stage 2, batch 1: native recognition and client-row contracts

- Base: 5dba48634d35fc7bf5bd545fa648c436add624e7
- 20 existing privacy-scrubbed order fixtures copied verbatim as JSON from the retired test catalog. No legacy parser executes. The Rust recognition suite now reads the JSON.
- Add institution address ownership and per-recipient product count/quantity checks to orders 5/16/17, restoring the missing legacy assertions.
- Add 10 independent native recognition cases for church, school, cathedral, hospital and academy as address suffixes or explicit recipients.
- Enable both native recognition sets in CI, with the seed and fixture paths watched; do not enable the historical differential module that invokes Node/TypeScript.
- Add four native client row preview tests: unchanged rows and preserved postcode/overrides, update/add and omitted client, invalid/duplicate ID and telephone collision, stale source revision.
- Client preview tests do not constitute an egui XLSX import/export integration test: the native row validator receives normalized cells. File-level formula and numeric telephone/zipcode guards, actual file roundtrip, transactional apply and current UI integration still require investigation.
- The remaining 808-test inventory, output/factory-reply/settlement expectations, OCR packaging and retired developer-only Rust tests remain open. No production code or legacy fixture is deleted.
- New CI execution is required; no pass claimed from static inspection.
