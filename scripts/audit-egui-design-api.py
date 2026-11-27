"""Conservative source-wide public design API reference inventory.

Run from any directory: python scripts/audit-egui-design-api.py [--output report.json]
This is a lexical reference audit, not rustc name resolution. A same-name
identifier in an unrelated module is deliberately counted as a potential use.
Never use this report alone to remove an API; review tests, reexports, macros,
and compile the proposed change.
"""
import argparse
from collections import defaultdict, Counter
import json
from pathlib import Path
import re
import runpy
import subprocess

SCRIPTS = Path(__file__).resolve().parent
REPO = SCRIPTS.parent
RUST_ROOT = REPO / "egui-app" / "src"
GUARD = runpy.run_path(str(SCRIPTS / "check-egui-layout-placement.py"))
surface = GUARD["design_public_surface"]
production = GUARD["production"]
strip_literals = GUARD["strip_literals"]
IDENTIFIER = re.compile(r"\b[A-Za-z_][A-Za-z0-9_]*\b")

def declaration_symbols(row):
    _, decl = row.split(" | ", 1)
    tokens = decl.split()
    i = tokens.index("pub") + 1
    if tokens[i] == "(":
        i = tokens.index(")", i) + 1
    if tokens[i] in ("const", "async", "unsafe") and tokens[i+1] == "fn":
        i += 1
    kind = tokens[i]
    if kind in ("fn", "const", "static", "struct", "enum", "trait", "union", "mod", "type"):
        names = [tokens[i+1]]
        if kind == "trait":
            names += re.findall(r"\bfn\s+([A-Za-z_]\w*)", decl)
        elif kind == "enum":
            # Count top-level enum variants as additional potential
            # references. Nested payload types are not variant names.
            body = tokens[tokens.index("{", i)+1:-1]
            depth, want_name, attrs = 0, True, 0
            for token in body:
                if token == "[":
                    attrs += 1
                elif token == "]":
                    attrs -= 1
                if attrs or token in ("#", "[", "]"):
                    continue
                if token in ("(", "{", "<"):
                    depth += 1
                elif token in (")", "}", ">") and depth:
                    depth -= 1
                elif token == "," and not depth:
                    want_name = True
                elif want_name and not depth and IDENTIFIER.fullmatch(token):
                    names.append(token)
                    want_name = False
        return kind, list(dict.fromkeys(names))
    if kind == "use":
        # Include source names and aliases. Wildcard reexports cannot be
        # safely resolved by name and must be reviewed manually.
        return "reexport", list(dict.fromkeys(t for t in tokens[i+1:] if IDENTIFIER.fullmatch(t) and t not in ("crate", "self", "super", "as")))
    return "field", [kind]

def owner_family(owner, path):
    base = "egui-app/src/" + owner
    return path == base or (owner.endswith("/mod.rs") and path.startswith(base[:-len("mod.rs")]))

def test_path(path):
    p = Path(path)
    return p.stem.endswith("_tests") or p.name in ("tests.rs", "ui_regressions.rs", "ui_test_support.rs") or "tests" in p.parts

def audit():
    inventory = GUARD["load_design_api"]()
    baseline = dict(inventory)
    inventory = {owner: surface((RUST_ROOT / owner).read_text(encoding="utf-8-sig"))
                 for owner in sorted(set(baseline) | set(GUARD["API_CONTRACT"]))
                 if (RUST_ROOT / owner).is_file()}
    drift = {owner: {"added": sorted((Counter(inventory.get(owner, []))-Counter(rows)).elements()),
                     "removed": sorted((Counter(rows)-Counter(inventory.get(owner, []))).elements())}
             for owner, rows in baseline.items()}
    drift = {owner: changes for owner, changes in drift.items() if changes["added"] or changes["removed"]}
    entries = []
    wanted = set()
    for owner, rows in sorted(inventory.items()):
        for row in rows:
            kind, names = declaration_symbols(row)
            entries.append((owner, row, kind, names))
            wanted.update(names)
    paths = subprocess.check_output(["git", "ls-files", "-z", "--", "*.rs"], cwd=REPO).decode("utf-8").split("\0")
    paths = sorted(p for p in paths if p)
    raw_hits = defaultdict(lambda: defaultdict(list))
    code_hits = defaultdict(lambda: defaultdict(list))
    for path in paths:
        source = (REPO / path).read_text(encoding="utf-8-sig")
        for line, text in enumerate(source.splitlines(), 1):
            for name in wanted.intersection(IDENTIFIER.findall(text)):
                raw_hits[name][path].append(line)
        if test_path(path) or re.search(r"#!\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]", source):
            continue
        for line, text in enumerate(production(source).splitlines(), 1):
            for name in wanted.intersection(IDENTIFIER.findall(text)):
                code_hits[name][path].append(line)

    report = []
    for owner, row, kind, names in entries:
        def references(index, internal):
            result = {}
            for name in names:
                for path, lines in index[name].items():
                    if owner_family(owner, path) == internal:
                        result.setdefault(path, {})[name] = lines
            return result
        external_code = references(code_hits, False)
        external_raw = references(raw_hits, False)
        internal_raw = references(raw_hits, True)
        if kind == "reexport" or "*" in row.split(" | ", 1)[1].split():
            status = "manual_reexport_review"
        elif external_code:
            status = "external_code_name_match"
        elif external_raw:
            status = "external_test_or_text_name_match"
        else:
            status = "owner_only_candidate"
        report.append({
            "owner": owner, "declaration": row, "kind": kind,
            "symbols": names, "classification": status,
            "external_code": external_code, "external_raw": external_raw,
            "owner_family_raw": internal_raw,
        })
    return {
        "method": "conservative lexical name matches; not resolved call sites; code includes some individually cfg(test)-gated items",
        "source_files": len(paths), "modules": len(inventory),
        "declarations": len(report), "summary": dict(Counter(row["classification"] for row in report)),
        "baseline_drift": drift,
        "entries": report,
    }

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    report = audit()
    summary = {k:v for k,v in report.items() if k != "entries"}
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2)+"\n", encoding="utf-8")

if __name__ == "__main__":
    main()
