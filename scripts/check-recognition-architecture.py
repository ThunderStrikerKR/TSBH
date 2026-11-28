"""Recognition integration architecture gate.

The rebuild is intentionally incomplete. A development pass means no
additional known phase violations, not that the integration is complete.
The main-bound gate additionally rejects the remaining legacy paths and
requires executable integration tests discovered by Cargo.

This source check covers named Rust functions and direct calls, not a
complete Rust call graph. Behavioral tests must establish actual semantics.
Do not increase debt budgets or rename tests to conceal an incomplete phase.
"""
import argparse
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CORE = ROOT / "src-tauri/src/business_core/recognition"
# Existing direct product decisions in the pre-feature extraction path.
# The standalone match_products API in execute is not document extraction.
LEGACY_BUDGET = {
    ("mod.rs::analyze", "search.decision"): 3,
    ("mod.rs::project_name", "search.decision"): 1,
    ("mod.rs::evaluate_product_probe", "model::evaluate"): 1,
}
FINAL_TESTS = (
    "common_candidates_preserve_competing_types_before_selection",
    "shared_document_features_are_built_once_and_reused_by_both_engines",
    "document_selection_runs_after_common_features",
    "explicit_business_constraints_survive_blending",
    "pattern_identity_is_independent_of_candidate_confidence",
    "pattern_b_is_invariant_under_pattern_a_growth",
    "confirmed_and_corrected_learning_contributions_are_retractable",
    "incompatible_or_unvalidated_learning_falls_back_to_baseline",
    "incremental_retraining_updates_only_changed_pattern",
    "final_combined_accuracy_and_resource_budget_are_verified",
)
CURRENT_TEST = "recognition::model::option_boundary_tests::collected_candidate_families_cover_evaluated_identities"


def production(source):
    # Current recognition files keep test modules at the end.
    return source.split("#[cfg(test)]", 1)[0]


def functions(source):
    # Top-level free functions only. A moved call that no longer belongs
    # to one of these named functions is charged to the unclassified scope.
    source = production(source)
    starts = list(re.finditer(
        r"(?m)^(?:pub(?:\([^)\n]*\))?\s+)?(?:async\s+)?fn\s+(\w+)\b",
        source,
    ))
    result = {}
    if not starts:
        return {"<unclassified>": source}
    result["<unclassified>"] = source[:starts[0].start()]
    for i, match in enumerate(starts):
        end = starts[i + 1].start() if i + 1 < len(starts) else len(source)
        name = match.group(1)
        if name in result:
            raise ValueError("Duplicate top-level function: " + name)
        result[name] = source[match.start():end]
    return result


def call_count(source, name):
    # Avoid matching the declaration, comments and ordinary quoted text.
    source = re.sub(r"(?m)//[^\n]*", "", source)
    source = re.sub(r'"(?:\\.|[^"\\])*"', '""', source)
    source = re.sub(r"\bfn\s+" + re.escape(name) + r"\b", "", source)
    return len(re.findall(r"(?<![\w:.])" + re.escape(name) + r"\s*\(", source))


def audit(sources, final=False):
    errors, debt = [], {}
    model = functions(sources["model.rs"])
    for name in ("collect", "evaluate", "decide"):
        if name not in model:
            errors.append("Missing product phase boundary: " + name)
    collector = model.get("collect", "")
    evaluator = model.get("evaluate", "")
    if not re.search(r"hypotheses\s*:\s*&\s*ProductHypotheses", evaluator):
        errors.append("Product evaluator must borrow original hypotheses")
    for name in ("decide", "evaluate", "strongest", "name_entries", "decision_json"):
        if call_count(collector, name):
            errors.append("Collector selects a product: " + name)
    for name in ("collect", "decide", "products", "evidence", "alias_entries",
                 "conductor_semantic_entries", "code_color_hint", "brand_words",
                 "known_brand"):
        if call_count(evaluator, name):
            errors.append("Evaluator discovers catalog candidates: " + name)
    for name in ("evidence", "alias_entries", "conductor_semantic_entries",
                 "code_color_hint", "brand_words"):
        if not call_count(collector, name):
            errors.append("Candidate family missing from collection: " + name)
    wrapper = model.get("decide", "")
    for name in ("collect", "evaluate"):
        if call_count(wrapper, name) != 1:
            errors.append("Standalone decide must reuse one collection and evaluation: " + name)

    for path, source in sources.items():
        if path == "model.rs":
            continue
        for name, body in functions(source).items():
            scope = path + "::" + name
            for call in ("model::decide", "search.decision", "model::evaluate"):
                count = call_count(body, call)
                if not count:
                    continue
                if scope == "mod.rs::execute" and call == "model::decide" and count == 1:
                    continue
                debt[scope + ":" + call] = count
                if count > LEGACY_BUDGET.get((scope, call), 0):
                    errors.append("New early product decision path: " + scope + ":" + call)
                if final:
                    errors.append("Unremoved legacy decision path: " + scope + ":" + call)
    return errors, debt


def self_test(sources):
    base, _ = audit(sources)
    assert not base, base
    mutations = [
        ("model.rs", "let ps=products(state);", "let ps=products(state); let _=decide(text,state,client_id);", "Collector selects"),
        ("model.rs", "hypotheses:&ProductHypotheses", "hypotheses:ProductHypotheses", "must borrow"),
        ("model.rs", "let proof = qualifiers(text);", "let proof = qualifiers(text); let _=products(state);", "Evaluator discovers"),
        ("mod.rs", "let client_id = client_id(client);", "let client_id = client_id(client); let _=model::decide(raw,state,client_id);", "New early"),
    ]
    # Mutations are checked only on the currently known migration layout.
    # When the old anchors disappear, the corresponding legacy debt must
    # disappear too; no mock production parser is installed.
    for path, before, after, expected in mutations:
        if before not in sources[path]:
            continue
        changed = dict(sources)
        changed[path] = changed[path].replace(before, after, 1)
        failures, _ = audit(changed)
        assert any(expected in f for f in failures), (expected, failures)
    if audit(sources)[1]:
        assert audit(sources, final=True)[0], "Final gate must reject legacy debt"


def verify_discovery(listed, ignored, final):
    tests = {line.removesuffix(": test") for line in listed.splitlines()
             if line.endswith(": test")}
    skipped = {line.removesuffix(": test") for line in ignored.splitlines()
               if line.endswith(": test")}
    required = [
        CURRENT_TEST,
        "common_candidates_preserve_competing_types_before_selection",
        "product_hypotheses_survive_evaluation_and_cache_reuse",
        "product_probe_limit_reports_incomplete_collection",
        "common_lexical_candidates_precede_document_evaluation",
    ]
    if final:
        required = list(dict.fromkeys(required + list(FINAL_TESTS)))
    failures = []
    for name in required:
        matches = [test for test in tests if test == name or test.endswith("::" + name)]
        if len(matches) != 1 or matches[0] in skipped:
            failures.append("Missing, duplicate or ignored required test: " + name)
    return failures


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--final", action="store_true")
    parser.add_argument("--tests", type=Path)
    parser.add_argument("--ignored", type=Path)
    args = parser.parse_args()
    sources = {p.relative_to(CORE).as_posix(): p.read_text(encoding="utf-8")
               for p in CORE.rglob("*.rs")}
    failures, debt = audit(sources, args.final)
    if not failures:
        self_test(sources)
    if args.tests and args.ignored:
        failures += verify_discovery(args.tests.read_text(), args.ignored.read_text(), args.final)
    elif args.final:
        failures.append("Final gate requires Cargo test and ignored-test discovery")
    for scope, count in sorted(debt.items()):
        print("OPEN architecture debt:", scope, count)
    for name in FINAL_TESTS:
        print("Required final behavior test:", name)
    if failures:
        for failure in failures:
            print("FAIL:", failure)
        raise SystemExit(1)
    print("PASS:", "final architecture gate" if args.final else
          "development architecture guard; integration NOT certified")


if __name__ == "__main__":
    main()
