"""Reject direct positioned UI builders and explicit allocations outside the shared layout owner.

This is a lexical guard for explicit rectangle placement, not a Rust type
checker. Native flow layout remains unrestricted; physical pixel helpers
are restricted to paint-only use. Test fixtures may use malformed coordinates.
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
    "floating_notice.rs", "navigation.rs", "category_tree/mod.rs", "one_touch.rs",
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


# Physical pixel rectangles are a paint implementation detail. This is a
# deliberately narrow source contract, not general Rust data-flow analysis.
# Only the popup owner may name these helpers; even there, the three mixed
# layout/paint functions must consume the snapped value directly in paint.
PIXEL_OWNER = "popup/mod.rs"
PIXEL_HELPERS = re.compile(
    r"\bmodal_master_(?:pixel_aligned_rect|rule_rect|toolbar_rule_rect)\b"
)
PIXEL_MIXED = {
    "modal_master_workspace_background",
    "modal_master_header_in_rect",
    "modal_master_header_with_toolbar_in_rect",
}
PIXEL_PAINT_ONLY = {
    "modal_master_workspace_finish",
    "paint_modal_master_footer_surface",
}
PIXEL_RECT_HELPER = "modal_master_pixel_aligned_rect"
PIXEL_RULE_HELPER = "modal_master_rule_rect"
PIXEL_TOOLBAR_HELPER = "modal_master_toolbar_rule_rect"

def function_regions(code, names=None):
    # These named popup functions have conventional signatures. Balance
    # braces after literal masking so nested paint/closure bodies stay intact.
    if names is None:
        names = PIXEL_MIXED | PIXEL_PAINT_ONLY | {
            PIXEL_RECT_HELPER, PIXEL_RULE_HELPER, PIXEL_TOOLBAR_HELPER,
        }
    for match in re.finditer(r"\bfn\s+(\w+)\b[^{};]*\{", code):
        name = match.group(1)
        if name not in names:
            continue
        depth = 1
        end = match.end()
        for brace in re.finditer(r"[{}]", code[end:]):
            depth += 1 if brace.group() == "{" else -1
            if depth == 0:
                end += brace.end()
                break
        else:
            raise ValueError("unterminated pixel paint function")
        yield name, match.start(), match.end(), end

def mask_matches(code, pattern):
    return re.sub(pattern, lambda m: mask(m.group()), code)

def pixel_paint_violations(name, code):
    findings = []
    def report(offset, rule):
        findings.append((code.count("\n", 0, offset) + 1, rule))
    if name != PIXEL_OWNER:
        for match in PIXEL_HELPERS.finditer(code):
            report(match.start(), "physical pixel helper outside popup paint owner")
        return findings

    remaining = code
    for fn, start, body_start, end in function_regions(code):
        body = code[body_start:end]
        if fn in PIXEL_MIXED:
            # Permit one explicit snapped binding, direct background paint,
            # and a divider expression directly inside a paint call. No
            # aliasing, derived layout sizes, return values or clip use.
            checked = mask_matches(body,
                r"\blet\s+paint_rect\s*=\s*modal_master_pixel_aligned_rect\s*\(\s*rect\s*,\s*ppp\s*\)\s*;")
            checked = mask_matches(checked,
                r"\bui\s*\.\s*painter\s*\(\s*\)\s*\.\s*rect_filled\s*\(\s*paint_rect\s*,")
            checked = mask_matches(checked,
                r"\bui\s*\.\s*painter\s*\(\s*\)\s*\.\s*rect_filled\s*\(\s*modal_master_rule_rect\s*\(\s*paint_rect\s*,\s*ppp\s*,\s*true\s*\)\s*,")
            checked = mask_matches(checked,
                r"\bui\s*\.\s*painter\s*\(\s*\)\s*\.\s*rect_filled\s*\(\s*modal_master_toolbar_rule_rect\s*\(\s*rect\s*,\s*ppp\s*\)\s*,")
            for match in re.finditer(r"\bpaint_rect\b", checked):
                report(body_start + match.start(), "physical paint rect escaped into non-paint expression")
        elif fn in PIXEL_PAINT_ONLY:
            # Paint-only helpers return unit and may touch Ui only to get a
            # painter or pixel scale. No Ui allocation, clipping, interaction,
            # delegation of Ui, or return of calculated physical geometry.
            signature = code[start:body_start]
            if not re.fullmatch(r"fn\s+\w+\s*\(\s*ui\s*:\s*&Ui\s*,\s*rect\s*:\s*Rect(?:\s*,\s*geometry\s*:\s*ModalMasterWorkspace)?\s*\)\s*\{", signature):
                report(start, "physical paint function must retain read-only Ui and unit return")
            checked = mask_matches(body,
                r"\bui\s*\.\s*(?:painter\s*\(\s*\)|ctx\s*\(\s*\)\s*\.\s*pixels_per_point\s*\(\s*\))")
            for match in re.finditer(r"\bui\b", checked):
                report(body_start + match.start(), "paint-only function used Ui outside painter or DPI access")
            checked = mask_matches(checked,
                r"\blet\s+rect\s*=\s*modal_master_pixel_aligned_rect\s*\(\s*rect\s*,\s*ppp\s*\)\s*;")
            checked = mask_matches(checked,
                r"\bmodal_master_rule_rect\s*\(\s*rect\s*,\s*ppp\s*,\s*false\s*\)")
        elif fn == PIXEL_TOOLBAR_HELPER:
            checked = mask_matches(body,
                r"\blet\s+rect\s*=\s*modal_master_pixel_aligned_rect\s*\(\s*rect\s*,\s*ppp\s*\)\s*;")
            checked = mask_matches(checked,
                r"\bmodal_master_rule_rect\s*\(\s*slot\s*,\s*ppp\s*,\s*false\s*\)")
        else:
            checked = body
        for match in PIXEL_HELPERS.finditer(checked):
            report(body_start + match.start(), "unapproved physical pixel helper use")
        # Rect-returning physical helpers must remain private.
        if fn in {PIXEL_RECT_HELPER, PIXEL_RULE_HELPER, PIXEL_TOOLBAR_HELPER}:
            if re.search(r"\bpub(?:\s*\([^)]*\))?\s*$", code[:start]):
                report(start, "physical pixel geometry helper must remain private")
        remaining = remaining[:start] + mask(remaining[start:end]) + remaining[end:]
    for match in PIXEL_HELPERS.finditer(remaining):
        report(match.start(), "physical pixel helper used outside approved paint function")
    return findings


# Shared physical-pixel math has a single owner. The only production
# consumers are the paint wrappers below; no import alias or layout caller.
PIXEL_MATH_OWNER = "ui_layout/pixel_paint.rs"
PIXEL_MATH_NAMES = r"(?:snap_paint_coordinate|snap_paint_rect|paint_stroke_width|centered_paint_axis_rect)"
PIXEL_MATH_REF = re.compile(r"\b(?:pixel_paint|" + PIXEL_MATH_NAMES + r")\b")
PIXEL_MATH_CALLS = {
    "popup/mod.rs": {
        PIXEL_RECT_HELPER: {"snap_paint_rect"},
        PIXEL_RULE_HELPER: {"paint_stroke_width"},
        "modal_master_workspace_finish": {"snap_paint_coordinate", "paint_stroke_width"},
    },
    "ui_layout.rs": {"paint_dock_axis_line": {"centered_paint_axis_rect"}},
}
DPI_READERS = {
    "popup/mod.rs": PIXEL_MIXED | PIXEL_PAINT_ONLY,
    "ui_layout.rs": {"paint_dock_axis_line"},
    # Native HWND boundary, not egui layout or paint geometry.
    "main.rs": {"kakao_pane"},
}
DPI_READ = re.compile(r"\b(?:pixels_per_point|native_pixels_per_point)\s*\(")
RAW_PIXEL_ROUND = re.compile(
    r"\b(?:round_to_pixel(?:s)?|round_to_pixel_center|snap_to_pixel(?:s)?)\b"
    r"|\*\s*(?P<scale>scale|ppp|dpi_scale|pixels_per_point)\s*\)\s*"
    r"\.\s*round\s*\(\s*\)(?:\s*\.\s*max\s*\([^)]*\))?\s*/\s*(?P=scale)\b"
    r"|\b(?P<reverse>scale|ppp|dpi_scale|pixels_per_point)\s*\*\s*[^;{}]+?\)\s*"
    r"\.\s*round\s*\(\s*\)(?:\s*\.\s*max\s*\([^)]*\))?\s*/\s*(?P=reverse)\b"
)
def pixel_math_violations(name, code):
    if name == PIXEL_MATH_OWNER:
        return []
    findings = []
    def report(offset, rule):
        findings.append((code.count("\n", 0, offset) + 1, rule))
    remaining = code
    dpi_remaining = code
    calls = PIXEL_MATH_CALLS.get(name, {})
    readers = DPI_READERS.get(name, set())
    for fn, start, body_start, end in function_regions(code, set(calls) | readers):
        body = code[body_start:end]
        allowed = calls.get(fn, set())
        if allowed:
            prefix = r"crate\s*::\s*ui_layout\s*::\s*" if name == "popup/mod.rs" else ""
            pattern = r"\b" + prefix + r"pixel_paint\s*::\s*(?:" + "|".join(sorted(allowed)) + r")\s*\("
            body = mask_matches(body, pattern)
            remaining = remaining[:body_start] + body + remaining[end:]
        if fn in readers:
            dpi_remaining = dpi_remaining[:body_start] + mask(dpi_remaining[body_start:end]) + dpi_remaining[end:]
    if name == "ui_layout.rs":
        remaining = mask_matches(remaining, r"\bpub\s*\(\s*crate\s*\)\s+mod\s+pixel_paint\s*;")
    for match in PIXEL_MATH_REF.finditer(remaining):
        report(match.start(), "shared physical pixel math used outside approved paint path")
    for match in DPI_READ.finditer(dpi_remaining):
        report(match.start(), "DPI read outside approved paint or native OS boundary")
    for match in RAW_PIXEL_ROUND.finditer(code):
        report(match.start(), "independent physical pixel rounding formula; use shared paint math")
    return findings

def violations(name, source):
    code = production(source)
    findings = pixel_paint_violations(name, code) + pixel_math_violations(name, code)
    owner_calls = 0
    if name == OWNER:
        for match in re.finditer(r"\b(?:crate|super)\s*::", code):
            findings.append((code.count("\n", 0, match.start()) + 1,
                             "geometry must not depend on application modules"))
    retired = (
        r"\bui_layout\s*::\s*(?:aligned_rect|AlignedUiBuilderExt|SharedAllocationExt|SharedUiClipExt|SharedPainterClipExt|set_fixed_height|reserve_min_height|reserve_min_size)\b",
        r"\bui_primitives\s*::\s*(?:CardWidthPolicy|UiWidthPolicyExt|apply_width_policy)\b",
        r"\bui_layout\s*::\s*\{[^}]*\b(?:aligned_rect|AlignedUiBuilderExt|SharedAllocationExt|SharedUiClipExt|SharedPainterClipExt|set_fixed_height|reserve_min_height|reserve_min_size)\b",
        r"\bui_primitives\s*::\s*\{[^}]*\b(?:CardWidthPolicy|UiWidthPolicyExt|apply_width_policy)\b",
    )
    for pattern in retired:
        for match in re.finditer(pattern, code):
            findings.append((code.count("\n", 0, match.start()) + 1,
                             "retired geometry forwarding path; use geometry directly"))
    if name in {"ui_layout.rs", "ui_primitives.rs"}:
        for match in re.finditer(r"\bpub(?:\s*\([^)]*\))?\s+use\s+(?:crate\s*::\s*ui_layout\s*::\s*)?geometry\s*::", code):
            findings.append((code.count("\n", 0, match.start()) + 1,
                             "geometry forwarding exports are prohibited"))
    if name in CORE_COMPONENTS:
        for match in re.finditer(r"\bcrate\s*::\s*ui_layout\s*::\s*(?!(?:geometry|pixel_paint)\b)\w+", code):
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
    assert violations("screen.rs", "use crate::ui_layout::SharedAllocationExt;")
    assert violations("screen.rs", "use crate::ui_primitives::UiWidthPolicyExt;")
    assert violations("screen.rs", "use crate::ui_layout::{fixed_row, SharedAllocationExt};")
    assert violations("screen.rs", "use crate::ui_primitives::{truncated_label, CardWidthPolicy};")
    assert not violations("screen.rs", "use crate::ui_layout::geometry::SharedAllocationExt;")
    assert violations("ui_layout.rs", "pub(crate) use geometry::{aligned_rect};")
    assert violations("ui_primitives.rs", "pub(crate) use crate::ui_layout::geometry::CardWidthPolicy;")
    assert violations(OWNER, "self.max_rect(aligned_rect(rect)); builder.max_rect(rect);")


    # Physical rectangles may be painted, but not fed into logical layout.
    def mixed(body):
        return "fn modal_master_header_in_rect() {" + body + "}"
    good = "let paint_rect=modal_master_pixel_aligned_rect(rect,ppp); ui.painter().rect_filled(paint_rect,0.0,color);"
    assert not violations(PIXEL_OWNER, mixed(good))
    assert not violations(PIXEL_OWNER, mixed(good + "ui.painter().rect_filled(modal_master_rule_rect(paint_rect,ppp,true),0.0,color);"))
    for bad in (
        "builder.aligned_max_rect(paint_rect);",
        "ui.restrict_shared_clip(paint_rect);",
        "ui.interact(paint_rect,id,sense);",
        "let alias=paint_rect; builder.aligned_max_rect(alias);",
        "ui.shared_allocate_exact_size(paint_rect.size(),sense);",
        "let width=paint_rect.width();",
        "return paint_rect;",
        "let rule=modal_master_rule_rect(rect,ppp,true);",
        "let rule=modal_master_toolbar_rule_rect(rect,ppp);",
        "let rect=modal_master_pixel_aligned_rect(rect,ppp);",
    ):
        assert violations(PIXEL_OWNER, mixed(good + bad)), bad
    assert violations("screen.rs", "use crate::popup::modal_master_pixel_aligned_rect as snap;")
    assert violations(PIXEL_OWNER, "fn layout() { modal_master_pixel_aligned_rect(rect,ppp); }")
    assert violations(PIXEL_OWNER, "pub(crate) fn modal_master_pixel_aligned_rect(rect:Rect,ppp:f32)->Rect { rect }")
    assert not violations(PIXEL_OWNER, "fn modal_master_pixel_aligned_rect(rect:Rect,ppp:f32)->Rect { rect }")
    paint = "fn paint_modal_master_footer_surface(ui:&Ui,rect:Rect) { let ppp=ui.ctx().pixels_per_point(); let rect=modal_master_pixel_aligned_rect(rect,ppp); ui.painter().rect_filled(rect,0.0,color);"
    assert not violations(PIXEL_OWNER, paint + "}")
    assert violations(PIXEL_OWNER, paint + "ui.interact(rect,id,sense);}")
    assert violations(PIXEL_OWNER, paint + "other(ui,rect);}")
    assert violations(PIXEL_OWNER, "fn paint_modal_master_footer_surface(ui:&Ui,rect:Rect)->Rect { rect }")
    assert not violations("screen.rs", "// modal_master_pixel_aligned_rect(rect,ppp)\n#[cfg(test)] mod t { fn f() { modal_master_rule_rect(rect,ppp,true); } }")
    assert not violations("main.rs", "fn kakao_pane() { let scale=ui.ctx().pixels_per_point(); dock.connect(region,scale); }")
    assert violations("main.rs", "fn layout() { let ppp=ui.ctx().pixels_per_point(); }")
    assert violations("screen.rs", "let x=(value*scale).round()/scale;")
    assert violations("screen.rs", "let x=(width*ppp).round().max(1.0)/ppp;")
    assert violations("screen.rs", "let x=(ppp*width).round()/ppp;")
    assert violations("screen.rs", "rect.round_to_pixel(ppp);")
    assert violations("screen.rs", "use crate::ui_layout::pixel_paint::snap_paint_rect as snap;")
    assert violations("popup/mod.rs", "fn modal_master_header_in_rect() { crate::ui_layout::pixel_paint::snap_paint_rect(rect,ppp); }")
    assert not violations("popup/mod.rs", "fn modal_master_pixel_aligned_rect(rect:Rect,pixels_per_point:f32)->Rect { crate::ui_layout::pixel_paint::snap_paint_rect(rect,pixels_per_point) }")
    assert not violations("ui_layout.rs", "fn paint_dock_axis_line() { let scale=ui.ctx().pixels_per_point(); let line=pixel_paint::centered_paint_axis_rect(rect,true,width,scale); }")
    assert not violations("ui_layout.rs", "pub(crate) mod pixel_paint;")
    assert not violations(PIXEL_MATH_OWNER, "let x=(value*scale).round()/scale;")
    assert not violations("kakao_dock.rs", "let to_px=|v:f32|->i32{(v*scale).round() as i32};")

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
