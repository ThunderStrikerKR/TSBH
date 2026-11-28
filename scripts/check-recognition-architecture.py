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


# Admission conditions are separate from the development debt budget.
PHASE02_TESTS = {
    "02-01": ("collected_candidate_families_cover_evaluated_identities",),
    "02-02": ("source_geometry_preserves_mixed_newlines_blank_lines_and_utf16",
              "physical_geometry_is_independent_of_record_partition",
              "document_candidates_keep_all_absolute_source_spans",
              "physical_line_contexts_are_partition_invariant"),
    "02-03": ("product_hypotheses_survive_evaluation_and_cache_reuse",
              "common_lexical_candidates_precede_document_evaluation",
              "competing_information_types_survive_legacy_projection"),
    "02-04": ("explicit_same_line_recipient_records_keep_distinct_source_offsets",
              "same_line_recipient_boundary_alternatives_survive_collection"),
    "02-05": ("same_line_bare_product_boundaries_are_not_identity_decisions",
              "same_line_product_boundary_alternatives_survive_collection"),
    "02-06": ("sender_candidates_survive_discovery_without_leaking_into_recipient_projection",
              "mixed_zone_boundary_alternatives_survive_collection"),
    "02-07": ("repeated_registered_terms_and_aliases_preserve_each_occurrence",
              "product_probe_limit_reports_incomplete_collection",
              "unrecognized_conflicting_and_excluded_candidates_are_preserved",
              "color_option_and_semantic_occurrences_are_preserved",
              "incomplete_document_candidates_are_ineligible_for_shared_features",
              "document_candidate_discovery_reuses_catalog_and_lexical_scans"),
}
PHASE02_MODULES = {"mod.rs", "model.rs", "quantity.rs", "ts_semantics.rs", "corrections.rs"}
LATER_PHASE_SYMBOLS = re.compile(
    r"\b(?:DocumentFeatures|document_features|build_document_features|"
    r"PatternIdentity|pattern_identity|document_pattern|role_cycle|"
    r"learning_context|observations)\b"
)
STAGE_FILE = ROOT / "scripts/recognition-stage.txt"


def stage_number(raw):
    raw = raw.strip()
    if raw not in {f"{n:02d}" for n in range(2, 10)}:
        raise ValueError("Invalid recognition stage; expected 02 through 09")
    return int(raw)


def admission_required(sources, stage, requested=False):
    # A new core module during phase 02 requires explicit architectural
    # review. Later-phase work cannot be hidden by leaving the stage at 02.
    return (requested or stage > 2 or bool(set(sources) - PHASE02_MODULES)
            or any(LATER_PHASE_SYMBOLS.search(production(source))
                   for source in sources.values()))


def phase02_debt(sources):
    """Audited source blockers, not a complete Rust call graph."""
    mod = production(sources["mod.rs"])
    model = production(sources["model.rs"])
    fn = functions(sources["mod.rs"])
    collect = fn.get("collect_document", "")
    analyze = fn.get("analyze", "")
    debt = []
    def flag(condition, task, reason):
        if condition:
            debt.append(f"{task}: {reason}")
    flag(bool(re.search(r"->\s*Vec\s*<\s*LineCandidates", collect)),
         "02-02", "collector returns record rows, not a unified document candidate batch")
    flag(bool(re.search(r"rows\s*\.\s*get\s*\(\s*i\s*\+\s*1|rows\s*\[\s*i\s*-\s*1", collect))
         or call_count(collect, "physical_line_contexts") != 1,
         "02-02", "candidate adjacency must use one physical-line context index")
    flag("row.note.is_none()" in collect or "row.explicit_name.is_none()" in collect
         or "row.zone!=SourceZone::Sender" in collect,
         "02-03", "legacy projection filters discovery by predicted information type")
    flag("address_candidate:Option<TextCandidate>" in mod,
         "02-03", "single address candidate excludes competing address ranges")
    flag("model_before_address(" in analyze or "quantity::extract(" in analyze,
         "02-03", "projection repeats lexical or catalog discovery")
    flag(bool(re.search(r"if\s+let\s+Some\(parts\)\s*=\s*recipient_records", collect)),
         "02-04", "recipient partition chosen before complete collection")
    flag(bool(re.search(r"if\s+let\s+Some\(parts\)\s*=\s*product_records", collect)),
         "02-05", "product partition chosen before complete collection")
    flag("sender_section" in collect, "02-06", "single sender state chooses region")
    flag("zone:SourceZone" in mod, "02-06", "record stores only a single zone")
    flag("color_hint:Option<" in model, "02-07", "color discovery keeps a single hint")
    flag("if e.recipient.is_none(){unassigned.push" in analyze,
         "02-07", "legacy unassigned projection is not unknown-span preservation")
    flag("self.probes.len()>=512" in mod and "truncated:bool" in mod,
         "02-07", "bounded cache needs a document completeness contract")
    return debt


def phase02_self_test():
    assert stage_number("02\n") == 2
    assert admission_required({}, 3)
    assert admission_required({}, 2, requested=True)
    assert admission_required({"new_core_module.rs": ""}, 2)
    assert admission_required({"mod.rs": "struct DocumentFeatures;"}, 2)
    assert not admission_required({"mod.rs": "fn collect_document() {}"}, 2)
    negative = phase02_debt({
        "mod.rs": "fn collect_document() -> Vec<LineCandidates<'_>> { if row.note.is_none() {} }",
        "model.rs": "struct ProductHypotheses { color_hint:Option<String> }",
    })
    for task in ("02-02", "02-03", "02-07"):
        assert any(x.startswith(task + ":") for x in negative), negative
    # Missing and ignored tests cannot count as certified admission.
    names = [name for group in PHASE02_TESTS.values() for name in group]
    assert verify_discovery("", "", False, True)
    ignored = names[-1] + ": test"
    assert any(names[-1] in x for x in verify_discovery(ignored, ignored, False, True))


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


def verify_discovery(listed, ignored, final, phase02=False):
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
        "repeated_registered_terms_and_aliases_preserve_each_occurrence",
        "physical_line_contexts_are_partition_invariant",
    ]
    if phase02 or final:
        required.extend(name for names in PHASE02_TESTS.values() for name in names)
    if final:
        required.extend(FINAL_TESTS)
    required = list(dict.fromkeys(required))
    failures = []
    for name in required:
        matches = [test for test in tests if test == name or test.endswith("::" + name)]
        if len(matches) != 1 or matches[0] in skipped:
            failures.append("Missing, duplicate or ignored required test: " + name)
    return failures


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--final", action="store_true")
    parser.add_argument("--phase02", action="store_true",
                        help="require admission to stage 03 regardless of declared stage")
    parser.add_argument("--tests", type=Path)
    parser.add_argument("--ignored", type=Path)
    args = parser.parse_args()
    sources = {p.relative_to(CORE).as_posix(): p.read_text(encoding="utf-8")
               for p in CORE.rglob("*.rs")}
    failures, debt = audit(sources, args.final)
    stage = stage_number(STAGE_FILE.read_text(encoding="utf-8"))
    require_phase02 = admission_required(sources, stage, args.phase02 or args.final)
    phase02_self_test()
    phase_debt = phase02_debt(sources)
    print(f"Declared recognition stage: {stage:02d}")
    for reason in phase_debt:
        print("OPEN phase-02 admission debt:", reason)
    for task, names in PHASE02_TESTS.items():
        for name in names:
            print("Required phase-02 behavior:", task, name)
    if require_phase02:
        failures.extend("Phase-02 admission blocked: " + reason for reason in phase_debt)
    if not failures:
        self_test(sources)
    if args.tests and args.ignored:
        listed, ignored = args.tests.read_text(), args.ignored.read_text()
        failures += verify_discovery(listed, ignored, args.final, require_phase02)
        missing = verify_discovery(listed, ignored, False, True)
        for reason in missing:
            print("OPEN phase-02 admission test:", reason)
        print("PHASE 02 ADMISSION:", "BLOCKED" if phase_debt or missing
              else "SOURCE AND DISCOVERY READY; CARGO REGRESSION PASS STILL REQUIRED")
    elif args.final or args.phase02:
        failures.append("Admission requires Cargo test and ignored-test discovery")
    else:
        print("PHASE 02 ADMISSION:", "BLOCKED" if phase_debt
              else "NOT CERTIFIED; test discovery and execution required")
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
