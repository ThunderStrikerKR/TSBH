"""Recognition integration behavioral test admission.

The original 78-item plan defines scope. Rust regression execution and
fixed-baseline comparisons are the evidence; file names, function names
and call counts are not architecture certification.
"""
import argparse
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
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


# Behavioral acceptance criteria for the existing phase 02 tasks.
PHASE02_TESTS = {
    "02-01": ("collected_candidate_families_cover_evaluated_identities",),
    "02-02": ("source_geometry_preserves_mixed_newlines_blank_lines_and_utf16",
              "physical_geometry_is_independent_of_record_partition",
              "document_candidates_keep_all_absolute_source_spans",
              "physical_line_contexts_are_partition_invariant"),
    "02-03": ("product_hypotheses_survive_evaluation_and_cache_reuse",
              "common_lexical_candidates_precede_document_evaluation",
              "competing_information_types_survive_legacy_projection",
              "competing_address_ranges_survive_legacy_projection"),
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
PHASE03_TESTS = (
    "common_document_features_preserve_candidates_and_order_before_evaluation",
    "exact_and_incomplete_repetition_preserve_source_boundaries",
    "pattern_identity_is_independent_of_candidate_confidence",
    "pattern_normalizes_item_multiplicity_without_discarding_source_candidates",
    "signed_distances_and_blank_boundaries_use_original_geometry",
)
PHASE04_TESTS = (
    "product_rank_preserves_ties_and_explicit_constraints",
    "baseline_candidate_trace_does_not_change_business_result",
    "baseline_quantity_constraints_reject_ambiguous_and_reused_evidence",
    "assignment_graph_rejects_invalid_source_and_owner",
    "address_format_is_not_directory_validation",
)
PHASE05_TESTS = (
    "normalized_weight_features_are_bounded_and_distance_decays",
    "experimental_weight_profiles_keep_version_and_component_bounds",
    "baseline_weight_experiment_report",
)
STAGE_FILE = ROOT / "scripts/recognition-stage.txt"


def stage_number(raw):
    raw = raw.strip()
    if raw not in {f"{n:02d}" for n in range(2, 10)}:
        raise ValueError("Invalid recognition stage; expected 02 through 09")
    return int(raw)


def verify_discovery(listed, ignored, final, phase02=False, phase03=False, phase04=False, phase05=False):
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
        "competing_information_types_survive_legacy_projection",
        "competing_address_ranges_survive_legacy_projection",
    ]
    if phase02 or final:
        required.extend(name for names in PHASE02_TESTS.values() for name in names)
    if phase03 or final:
        required.extend(PHASE03_TESTS)
    if phase04 or final:
        required.extend(PHASE04_TESTS)
    if phase05 or final:
        required.extend(PHASE05_TESTS)
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
    stage = stage_number(STAGE_FILE.read_text(encoding="utf-8"))
    require_phase02 = args.phase02 or args.final or stage > 2
    print(f"Declared recognition stage: {stage:02d}")
    if not (args.tests and args.ignored):
        if require_phase02:
            raise SystemExit("Admission requires Cargo test and ignored-test discovery")
        print("Test discovery and execution required; integration not certified")
        return
    listed, ignored = args.tests.read_text(), args.ignored.read_text()
    failures = verify_discovery(listed, ignored, args.final, require_phase02, stage >= 3, stage >= 4, stage >= 5)
    if failures:
        for failure in failures:
            print("FAIL:", failure)
        raise SystemExit(1)
    if not require_phase02:
        missing = verify_discovery(listed, ignored, False, True)
        if missing:
            print("Phase 02 pending behavioral coverage:", len(missing))
    print("PASS: required test discovery; regression execution and review still required")


if __name__ == "__main__":
    main()
