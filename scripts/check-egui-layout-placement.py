"""Reject direct positioned UI builders and explicit allocations outside the shared layout owner.

This is a lexical guard for explicit rectangle placement, not a Rust type
checker. Native flow layout and paint-only physical-pixel snapping are not
restricted. Test-only fixtures may deliberately use malformed coordinates.
"""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1] / "egui-app" / "src"
OWNER = "ui_layout/geometry.rs"
CORE_COMPONENTS = {
    "theme.rs", "ui_primitives.rs", "card/mod.rs", "table/mod.rs",
    "popup/mod.rs", "popup/tag.rs", "input/mod.rs", "button/mod.rs",
    "checkbox/mod.rs", "dropdown/mod.rs", "status_notice/mod.rs",
}

# Preserve line numbers while removing comments and string/character literals.
# Rust block comments can nest; consume them explicitly.
TOKEN = re.compile(r'//[^\n]*|/\*|(?:b|c)?r(?P<hash>#{0,255})"[\s\S]*?"(?P=hash)|(?:b|c)?"(?:\\[\s\S]|[^"\\])*"|(?:b)?\x27(?:\\.|[^\x27\\\n])\x27')

def mask(text):
    return "".join("\n" if c == "\n" else " " for c in text)

def strip_literals(source):
    result = []
    pos = 0
    while match := TOKEN.search(source, pos):
        result.append(source[pos:match.start()])
        end = match.end()
        if match.group() == "/*":
            depth = 1
            while depth:
                nested = re.search(r"/\*|\*/", source[end:])
                if not nested:
                    raise ValueError("unterminated Rust block comment")
                end += nested.end()
                depth += 1 if nested.group() == "/*" else -1
        result.append(mask(source[match.start():end]))
        pos = end
    result.append(source[pos:])
    return "".join(result)

def production(source):
    # Remove cfg(test) module bodies using balanced braces after lexical
    # masking. Preserve code after inline test modules and line numbers.
    source = strip_literals(source)
    pattern = re.compile(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*(?:#\s*\[[^\]]*\]\s*)*(?:pub(?:\s*\([^)]*\))?\s+)?mod\s+\w+\s*\{")
    while match := pattern.search(source):
        depth = 1
        end = match.end()
        for brace in re.finditer(r"[{}]", source[end:]):
            depth += 1 if brace.group() == "{" else -1
            if depth == 0:
                end += brace.end()
                break
        else:
            raise ValueError("unterminated cfg(test) module")
        source = source[:match.start()] + mask(source[match.start():end]) + source[end:]
    return source

RULES = (
    ("raw UiBuilder.max_rect", re.compile(r"\.\s*max_rect\s*\(\s*(?![\s)])")),
    ("associated UiBuilder.max_rect", re.compile(r"\bUiBuilder\s*::\s*max_rect\s*\(")),
    ("direct max_rect assignment", re.compile(r"\.\s*max_rect\s*=(?!=)")),
    ("UiBuilder struct max_rect", re.compile(r"\bUiBuilder\s*\{[^{}]*\bmax_rect\s*:")),
    ("raw positioned UI helper", re.compile(r"\.\s*(?:place|allocate_ui_at_rect|allocate_new_ui)\s*\(")),
    ("associated positioned UI helper", re.compile(r"\bUi\s*::\s*(?:place|put|allocate_ui_at_rect|allocate_new_ui)\s*\(")),
    ("raw UI allocation", re.compile(r"\.\s*(?:allocate_exact_size|allocate_space|allocate_ui_with_layout|allocate_at_least|add_sized|allocate_response|allocate_painter|allocate_ui)\s*(?:::<[^>]*>\s*)?\(")),
    ("associated UI allocation", re.compile(r"\bUi\s*::\s*(?:allocate_exact_size|allocate_space|allocate_ui_with_layout|allocate_at_least|add_sized|allocate_response|allocate_painter|allocate_ui)\s*\(")),
    ("raw clip API", re.compile(r"\.\s*(?:set_clip_rect|with_clip_rect|shrink_clip_rect)\s*\(")),
    ("associated clip API", re.compile(r"\b(?:Ui|Painter)\s*::\s*(?:set_clip_rect|with_clip_rect|shrink_clip_rect)\s*\(")),
    ("owner pane clip replacement", re.compile(r"\.\s*use_owner_pane_clip\s*\(")),
    ("raw Ui.put", re.compile(r"\b(?:ui|root|pane|child|row|body|host|slot|cell|panel|content|parent|control)\s*\.\s*put\s*\(")),
)

def violations(name, source):
    code = production(source)
    findings = []
    owner_calls = 0
    if name == OWNER:
        for match in re.finditer(r"\b(?:crate|super)\s*::", code):
            findings.append((code.count("\n", 0, match.start()) + 1,
                             "geometry must not depend on application modules"))
    if name in CORE_COMPONENTS:
        for match in re.finditer(r"\bcrate\s*::\s*ui_layout\s*::\s*(?!geometry\b)\w+", code):
            findings.append((code.count("\n", 0, match.start()) + 1,
                             "component must use geometry, not layout composition"))
    for rule, regex in RULES:
        for match in regex.finditer(code):
            if name == OWNER and rule == "raw UiBuilder.max_rect":
                prefix = code[max(0, match.start()-5):match.start()]
                tail = code[match.start():]
                if prefix.endswith("self") and re.match(r"\.\s*max_rect\s*\(\s*aligned_rect\s*\(\s*rect\s*\)\s*\)", tail):
                    owner_calls += 1
                    continue
            if name == OWNER and rule == "raw UI allocation":
                prefix = code[max(0, match.start()-5):match.start()]
                tail = code[match.start():]
                if prefix.endswith("self") and re.match(r"\.\s*(?:allocate_exact_size|allocate_space|allocate_ui_with_layout|add_sized|allocate_response)\s*\(\s*aligned_allocation_size\s*\(", tail):
                    continue
            if rule == "raw clip API" and name == OWNER:
                prefix = code[max(0, match.start()-5):match.start()]
                tail = code[match.start():]
                if prefix.endswith("self") and re.match(r"\.\s*(?:set_clip_rect\s*\(\s*(?:self\.clip_rect\(\)\.intersect\(requested\)|owner_pane)\s*\)|with_clip_rect\s*\(\s*requested\s*\))", tail):
                    continue
            if rule == "owner pane clip replacement" and name == "workflow_board_ui.rs":
                if re.match(r"\.\s*use_owner_pane_clip\s*\(\s*pane_clip\s*\)", code[match.start():]):
                    continue
            findings.append((code.count("\n", 0, match.start()) + 1, rule))
    if name == OWNER and owner_calls != 1:
        findings.append((1, "expected exactly one raw builder owner call"))
    return sorted(findings)

def self_test():
    assert not violations("screen.rs", "let x=ui.max_rect(); let y=ui.max_rect( );")
    assert not violations("screen.rs", "builder.aligned_max_rect(rect);")
    assert not violations("screen.rs", '// builder.max_rect(rect)\nlet s="builder.max_rect(rect)";')
    assert not violations("screen.rs", '#[cfg(test)] mod t { fn f() { builder.max_rect(bad); } }')
    assert len(violations("screen.rs", '#[cfg(test)] mod t { fn f() { builder.max_rect(bad); } }\nbuilder.max_rect(rect);')) == 1
    assert violations("screen.rs", "builder . max_rect (\n rect );")
    assert violations("screen.rs", "UiBuilder::max_rect(builder, rect);")
    assert violations("screen.rs", "builder.max_rect = Some(rect);")
    assert violations("screen.rs", "ui.place(rect, widget);")
    assert violations("screen.rs", "ui.put(rect, widget);")
    assert not violations("screen.rs", "states.put(id, state);")
    for api in ("set_clip_rect", "with_clip_rect", "shrink_clip_rect"):
        assert violations("screen.rs", f"ui.{api}(rect);")
        assert violations("screen.rs", f"Painter::{api}(painter, rect);")
    assert not violations("screen.rs", "ui.restrict_shared_clip(rect); painter.with_shared_clip(rect);")
    assert violations("screen.rs", "ui.use_owner_pane_clip(pane_clip);")
    assert not violations("workflow_board_ui.rs", "ui.use_owner_pane_clip(pane_clip);")
    assert violations("workflow_board_ui.rs", "ui.use_owner_pane_clip(other_clip);")
    assert not violations(OWNER, "self.max_rect(aligned_rect(rect)); self.set_clip_rect(self.clip_rect().intersect(requested)); self.set_clip_rect(owner_pane); self.with_clip_rect(requested);")
    assert violations(OWNER, "self.max_rect(aligned_rect(rect)); self.set_clip_rect(rect);")
    assert not violations("screen.rs", "ui.shared_allocate_exact_size(size, sense);")
    for api in ("allocate_exact_size", "allocate_space", "allocate_ui_with_layout",
                "allocate_at_least", "add_sized", "allocate_response", "allocate_painter", "allocate_ui"):
        assert violations("screen.rs", f"ui.{api}(size);")
        assert violations("screen.rs", f"Ui::{api}(ui, size);")
    assert not violations("screen.rs", "#[cfg(test)] mod t { fn f() { ui.allocate_exact_size(size, sense); } }")
    assert not violations(OWNER, "self.max_rect(aligned_rect(rect)); self.allocate_exact_size(aligned_allocation_size(size),sense);")
    assert violations(OWNER, "self.max_rect(aligned_rect(rect)); self.allocate_exact_size(size,sense);")
    assert not violations(OWNER, "self.max_rect(aligned_rect(rect))")
    assert violations(OWNER, "self.max_rect(aligned_rect(rect)); use crate::card::CardHeightPolicy;")
    assert violations("card/mod.rs", "use crate::ui_layout::SharedAllocationExt;")
    assert not violations("card/mod.rs", "use crate::ui_layout::geometry::SharedAllocationExt;")
    assert not violations("screen.rs", "use crate::ui_layout::SharedAllocationExt;")
    assert violations(OWNER, "self.max_rect(aligned_rect(rect)); builder.max_rect(rect);")

def main():
    self_test()
    failures = []
    checked = 0
    for path in sorted(ROOT.rglob("*.rs")):
        name = path.relative_to(ROOT).as_posix()
        source = path.read_text(encoding="utf-8-sig")
        if path.name in {"ui_regressions.rs", "ui_test_support.rs", "tests.rs"} or path.stem.endswith("_tests") or re.search(r"#!\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]", source):
            continue
        checked += 1
        failures.extend(f"{name}:{line}: {rule}" for line, rule in violations(name, source))
    for finding in failures:
        print(finding, file=sys.stderr)
    print(f"egui explicit placement ownership: {checked} Rust files, {len(failures)} violation(s)")
    return int(bool(failures))

if __name__ == "__main__":
    sys.exit(main())
