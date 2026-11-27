"""Reject direct business drag/drop primitives outside the shared owner.

Lexical guard, not a Rust type checker. Existing native window and text
gestures have per-function, per-token count limits.
"""
from collections import Counter
from pathlib import Path
import re
import runpy
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent / "egui-app" / "src"
COMMON = runpy.run_path(str(HERE / "check-egui-layout-placement.py"))
production = COMMON["production"]
function_regions = COMMON["function_regions"]
OWNER = "drag_drop.rs"
RAW = re.compile(
    r"\b(?:dnd_[A-Za-z_][A-Za-z0-9_]*|DragAndDrop|click_and_drag|"
    r"drag_started(?:_by)?|drag_stopped(?:_by)?|dragged(?:_by)?|"
    r"drag_delta|total_drag_delta|drag_motion|set_dragged_id|dragged_id|"
    r"stop_dragging|is_being_dragged|drag_painter|page_drag_painter|"
    r"DropTarget|DropHovered|Grabbing|NotAllowed)\b"
    r"|\bSense\s*::\s*drag\b"
)
EXCEPTIONS = {
    "main.rs": {
        "handle_header_drag": {"drag_started": 1},
        "top_bar": {"click_and_drag": 2},
        "reception": {"click_and_drag": 1, "dragged": 3,
                      "drag_started": 1, "drag_stopped": 1, "total_drag_delta": 1},
        "source_and_message": {"click_and_drag": 1, "dragged": 3,
                               "drag_started": 1, "drag_stopped": 1, "total_drag_delta": 1},
        "other_page": {"click_and_drag": 1, "dragged": 3,
                       "drag_started": 1, "drag_stopped": 1, "total_drag_delta": 1},
    },
    "input/mod.rs": {"management_search_append_click": {"dragged": 1}},
    "overlay.rs": {"drag_painter": {"drag_painter": 1},
                   "page_drag_painter": {"page_drag_painter": 1}},
}
HIGHLIGHT = {"DropTarget": 3, "DropHovered": 3}

def violations(name, source):
    if name == OWNER:
        return []
    code = production(source)
    allowances = EXCEPTIONS.get(name, {})
    regions = list(function_regions(code, set(allowances)))
    remaining = Counter()
    found = []
    for match in RAW.finditer(code):
        token = re.sub(r"\s+", "", match.group())
        if name == "highlight/mod.rs" and token in HIGHLIGHT:
            key = ("palette", token)
            remaining[key] += 1
            if remaining[key] <= HIGHLIGHT[token]:
                continue
        else:
            region = next((fn for fn, start, _, end in regions
                           if start <= match.start() < end), None)
            if region is not None:
                key = (region, token)
                remaining[key] += 1
                if remaining[key] <= allowances[region].get(token, 0):
                    continue
        found.append((code.count("\n", 0, match.start())+1,
                      f"direct drag/drop API {token}; use crate::drag_drop"))
    return found

def self_test():
    assert not violations("new_screen.rs", "fn show(){crate::drag_drop::start_native(ui,&response,true,|| payload);}")
    for bad in [
        "fn show(){response.dnd_set_drag_payload(data);}",
        "fn show(){response.dnd_release_payload::<T>();}",
        "fn show(){ui.dnd_drop_zone::<T,_>(frame,body);}",
        "use egui::DragAndDrop as D;",
        "use egui::Sense::drag as direct_drag;",
        "let sense=Sense :: click_and_drag ();",
        "let state=Response::dragged;",
        "let paint=crate::overlay::page_drag_painter;",
        "let role=highlight::Role::DropHovered;",
        "ctx.set_cursor_icon(CursorIcon::Grabbing);",
    ]:
        assert violations("new_screen.rs", bad), bad
    assert not violations(OWNER, "fn a(){response.dnd_set_drag_payload(data);}")
    assert not violations("new_screen.rs", '// DragAndDrop\nlet s="dnd_set_drag_payload";')
    assert not violations("new_screen.rs", "#[cfg(test)] mod tests {fn t(){response.dragged();}}")
    assert violations("new_screen.rs", "#[cfg(test)] mod tests{} fn render(){response.dragged();}")
    assert not violations("main.rs", "fn handle_header_drag(){response.drag_started();}")
    assert violations("main.rs", "fn handle_header_drag(){response.drag_started();response.drag_started();}")
    assert violations("main.rs", "fn new_action(){response.drag_started();}")
    assert violations("input/mod.rs", "fn unrelated(){response.dragged();}")
    assert violations("overlay.rs", "fn new_action(){page_drag_painter(ctx,owner,id);}")

# Existing out-of-line test modules declared under cfg(test).
TEST_FILES = {"ui_regressions.rs", "ui_test_support.rs",
              "table/regression_tests.rs", "button/regression_tests.rs",
              "card/regression_tests.rs", "checkbox/regression_tests.rs",
              "scrollbar/regression_tests.rs", "input/regression_tests.rs",
              "dropdown/regression_tests.rs", "popup/regression_tests.rs"}

def main():
    self_test()
    failures = []
    checked = 0
    if not (ROOT / OWNER).is_file():
        failures.append(f"{OWNER}:1: shared drag owner missing")
    for path in sorted(ROOT.rglob("*.rs")):
        source = path.read_text(encoding="utf-8-sig")
        name = path.relative_to(ROOT).as_posix()
        masked = COMMON["strip_literals"](source)
        if name in TEST_FILES or re.match(r"\s*#!\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]", masked):
            continue
        checked += 1
        failures.extend(f"{name}:{line}: {rule}" for line,rule in violations(name,source))
    for failure in failures:
        print(failure,file=sys.stderr)
    print(f"egui shared drag/drop ownership: {checked} Rust files, {len(failures)} violation(s)")
    return int(bool(failures))

if __name__ == "__main__":
    sys.exit(main())
