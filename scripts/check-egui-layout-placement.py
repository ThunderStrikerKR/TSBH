"""Reject direct positioned UI builders and explicit allocations outside the shared layout owner.

This is a lexical guard for explicit rectangle placement, not a Rust type
checker. Native flow layout remains unrestricted; physical pixel helpers
are restricted to paint-only use. Test fixtures may use malformed coordinates.
"""
from pathlib import Path
from collections import Counter
import json
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


# Pixel paint has a single owner. Removed forwarding APIs must not return.
PIXEL_OWNER = "popup/mod.rs"
RETIRED_APIS = re.compile(r"\b(?:modal_master_(?:pixel_aligned_rect|rule_rect|toolbar_rule_rect)|apply_width_policy)\b")
PIXEL_MIXED = {
    "modal_master_workspace_background",
    "modal_master_header_in_rect",
    "modal_master_header_with_toolbar_in_rect",
}
PIXEL_PAINT_ONLY = {
    "modal_master_workspace_finish",
    "paint_modal_master_footer_surface",
}

def function_regions(code, names):
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
    for match in RETIRED_APIS.finditer(code):
        report(match.start(), "retired forwarding API; use the owning module directly")
    if name != PIXEL_OWNER:
        return findings
    for fn, start, body_start, end in function_regions(code, PIXEL_MIXED | PIXEL_PAINT_ONLY):
        body = code[body_start:end]
        if fn in PIXEL_MIXED:
            checked = mask_matches(body,
                r"\blet\s+paint_rect\s*=\s*crate\s*::\s*ui_layout\s*::\s*pixel_paint\s*::\s*snap_paint_rect\s*\(\s*rect\s*,\s*ppp\s*\)\s*;")
            checked = mask_matches(checked,
                r"\bpaint_rect\s*\.\s*fill\s*\(\s*ui\s*\.\s*painter\s*\(\s*\)\s*,")
            checked = mask_matches(checked,
                r"\bpaint_rect\s*\.\s*edge_rule\s*\(\s*theme::SOFT_DIVIDER_WIDTH\s*,\s*theme::MODAL_MASTER_CONTENT_INSET\s*,\s*ppp\s*,\s*true\s*\)\s*\.\s*fill\s*\(\s*ui\s*\.\s*painter\s*\(\s*\)\s*,")
            checked = mask_matches(checked,
                r"\bpaint_rect\s*\.\s*toolbar_rule\s*\(\s*theme::MODAL_HEADER_HEIGHT\s*,\s*theme::SOFT_DIVIDER_WIDTH\s*,\s*theme::MODAL_MASTER_CONTENT_INSET\s*,\s*ppp\s*\)\s*\.\s*fill\s*\(\s*ui\s*\.\s*painter\s*\(\s*\)\s*,")
            for match in re.finditer(r"\b(?:paint_rect|snap_paint_rect)\b", checked):
                report(body_start + match.start(), "physical paint rect escaped approved paint expression")
        else:
            signature = code[start:body_start]
            if not re.fullmatch(r"fn\s+\w+\s*\(\s*ui\s*:\s*&Ui\s*,\s*rect\s*:\s*Rect(?:\s*,\s*geometry\s*:\s*ModalMasterWorkspace)?\s*\)\s*\{", signature):
                report(start, "physical paint function must retain read-only Ui and unit return")
            checked = mask_matches(body,
                r"\bui\s*\.\s*(?:painter\s*\(\s*\)|ctx\s*\(\s*\)\s*\.\s*pixels_per_point\s*\(\s*\))")
            for match in re.finditer(r"\bui\b", checked):
                report(body_start + match.start(), "paint-only function used Ui outside painter or DPI access")
    return findings

# Shared physical-pixel math has a single owner. The only production
# consumers are the paint wrappers below; no import alias or layout caller.
PIXEL_MATH_OWNER = "ui_layout/pixel_paint.rs"
PIXEL_MATH_NAMES = r"(?:snap_paint_coordinate|snap_paint_rect|paint_stroke_width|centered_paint_axis_rect)"
PIXEL_MATH_REF = re.compile(r"\b(?:pixel_paint|" + PIXEL_MATH_NAMES + r")\b")
PIXEL_MATH_CALLS = {
    "popup/mod.rs": {name: {"snap_paint_rect"} for name in PIXEL_MIXED | PIXEL_PAINT_ONLY},
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
        findings = []
        for match in re.finditer(r"\bpub(?:\s*\([^)]*\))?\s+fn\s+\w+\s*\([^{};]*?\)\s*->\s*(?:Rect|f32|&\s*Rect)\b", code):
            prefix = code[max(0,match.start()-100):match.start()]
            if re.search(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*$",prefix):
                continue
            findings.append((code.count("\n",0,match.start())+1,"paint type exposes raw layout geometry"))
        for match in re.finditer(r"\bimpl\b[^{};]*(?:Deref|AsRef\s*<\s*Rect|From\s*<\s*PaintRect|Into\s*<\s*Rect)[^{};]*\{",code):
            findings.append((code.count("\n",0,match.start())+1,"paint type must not implement a layout conversion"))
        for match in re.finditer(r"\bstruct\s+PaintRect\s*\(\s*pub\b",code):
            findings.append((code.count("\n",0,match.start())+1,"paint rectangle field must remain private"))
        return findings
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


# Public API contract: exact declaration tokens, independent of whitespace,
# comments and function bodies. A new, removed or changed public signature
# requires an explicit contract update. Trait members and enum variants are
# included because they are public without a pub keyword.
API_CONTRACT = {
    OWNER: [
        ("module", "pub(crate) fn aligned_rect(rect:Rect)->Rect {"),
        ("module", "pub(crate) fn set_fixed_height(ui:&mut Ui,height:f32){"),
        ("module", "pub(crate) fn reserve_min_height(ui:&mut Ui,height:f32){"),
        ("module", "pub(crate) fn reserve_min_size(ui:&mut Ui,size:Vec2){"),
        ("module", "pub(crate) trait AlignedUiBuilderExt { fn aligned_max_rect(self,rect:Rect)->Self; }"),
        ("module", """pub(crate) trait SharedAllocationExt {
            fn shared_allocate_exact_size(&mut self,size:Vec2,sense:Sense)->(Rect,egui::Response);
            fn shared_allocate_space(&mut self,size:Vec2)->(egui::Id,Rect);
            fn shared_allocate_ui_with_layout<R>(&mut self,size:Vec2,layout:Layout,draw:impl FnOnce(&mut Ui)->R)->egui::InnerResponse<R>;
            fn shared_add_sized(&mut self,size:impl Into<Vec2>,widget:impl egui::Widget)->egui::Response;
            fn shared_allocate_response(&mut self,size:Vec2,sense:Sense)->egui::Response;
        }"""),
        ("module", """pub(crate) trait SharedUiClipExt {
            fn restrict_shared_clip(&mut self,requested:Rect);
            fn use_owner_pane_clip(&mut self,owner_pane:Rect);
        }"""),
        ("module", "pub(crate) trait SharedPainterClipExt { fn with_shared_clip(&self,requested:Rect)->egui::Painter; }"),
        ("module", """pub(crate) enum CardWidthPolicy {
            #[default] FitContent, FillAvailable, Fixed, Bounded, UpperLimit,
        }"""),
        ("module", "pub(crate) trait UiWidthPolicyExt { fn apply_shared_width(&mut self,policy:CardWidthPolicy,target:f32); }"),
    ],
    PIXEL_MATH_OWNER: [
        ("module", "pub(crate) struct PaintRect(Rect);"),
        ("module", "pub(crate) fn snap_paint_rect(rect:Rect,pixels_per_point:f32)->PaintRect {"),
        ("module", "pub(crate) fn centered_paint_axis_rect(rect:Rect,vertical:bool,logical_width:f32,pixels_per_point:f32)->PaintRect {"),
        ("PaintRect", "pub(crate) fn fill(self,painter:&Painter,radius:impl Into<CornerRadius>,color:Color32){"),
        ("PaintRect", "pub(crate) fn edge_rule(self,logical_width:f32,inset:f32,ppp:f32,at_bottom:bool)->Self{"),
        ("PaintRect", "pub(crate) fn toolbar_rule(self,title_height:f32,logical_width:f32,inset:f32,ppp:f32)->Self{"),
        ("PaintRect", """pub(crate) fn finish_frame(self,painter:&Painter,ppp:f32,toolbar_bottom:f32,divider_x:Option<f32>,
            divider_width:f32,frame_width:f32,radius:impl Into<CornerRadius>,color:Color32){"""),
        ("PaintRect", "#[cfg(test)] pub(crate) fn test_rect(self)->Rect {"),
    ],
}
API_TOKEN = re.compile(r"[A-Za-z_][A-Za-z0-9_]*|\d+|::|->|[^\s]")
API_EXPORT_NAMES = re.compile(
    r"\b(?:geometry|pixel_paint|aligned_rect|set_fixed_height|reserve_min_height|reserve_min_size|"
    r"AlignedUiBuilderExt|SharedAllocationExt|SharedUiClipExt|SharedPainterClipExt|"
    r"CardWidthPolicy|UiWidthPolicyExt|PaintRect|snap_paint_rect|centered_paint_axis_rect)\b"
)

def api_tokens(text):
    return [m.group() for m in API_TOKEN.finditer(text)]

def api_contract_violations(name, source):
    code = production(source)
    findings = []
    def report(offset, rule):
        findings.append((code.count("\n", 0, offset) + 1, rule))
    # No direct or grouped public forwarding imports from the two owners.
    # Ordinary private imports remain valid.
    for match in re.finditer(r"\bpub(?:\s*\([^)]*\))?\s+use\s+[^;]+;", code):
        if API_EXPORT_NAMES.search(match.group()):
            report(match.start(), "shared geometry or pixel paint re-export prohibited")
    if name == "ui_layout.rs":
        for module in ("geometry", "pixel_paint"):
            pattern = r"\bpub\s*\(\s*crate\s*\)\s+mod\s+" + module + r"\s*;"
            if len(re.findall(pattern, code)) != 1:
                report(0, f"expected exactly one crate-visible {module} module declaration")
    if name not in API_CONTRACT:
        return findings

    matches = list(API_TOKEN.finditer(code))
    tokens = [m.group() for m in matches]
    # Track braces rather than treating all pub fn declarations as free
    # functions. PaintRect's approved methods must belong to its own impl.
    scopes = []
    scope_at = []
    for i, token in enumerate(tokens):
        scope_at.append(tuple(scopes))
        if token == "{":
            scopes.append(i)
        elif token == "}":
            if not scopes:
                report(matches[i].start(), "unbalanced API contract scope")
                return findings
            scopes.pop()
    if scopes:
        report(0, "unbalanced API contract scope")
        return findings

    covered = set()
    for scope, declaration in API_CONTRACT[name]:
        expected = api_tokens(declaration)
        candidates = []
        # Test-only marker is part of its declaration, not a separate
        # allowlist exception. Removing it therefore fails the contract.
        for i in range(len(tokens) - len(expected) + 1):
            if tokens[i:i+len(expected)] != expected:
                continue
            parents = scope_at[i]
            if scope == "module":
                valid = not parents
            else:
                valid = len(parents) == 1 and tokens[parents[0]-2:parents[0]] == ["impl", scope]
            if valid:
                candidates.append(i)
        if len(candidates) != 1:
            report(0, f"API contract requires exactly one {scope} declaration: {declaration.strip()}")
            continue
        i = candidates[0]
        covered.update(j for j in range(i, i+len(expected)) if tokens[j] == "pub")
    for i, token in enumerate(tokens):
        if token == "pub" and i not in covered:
            report(matches[i].start(), "unapproved public API or visibility; update the owner contract explicitly")
    return findings

def api_contract_self_test():
    # Synthetic complete declarations keep tests independent of the current
    # owner files; production is checked separately below.
    def fixture(name):
        top, methods = [], []
        for scope, decl in API_CONTRACT[name]:
            line = decl + (" }" if decl.endswith("{") else "")
            (top if scope == "module" else methods).append(line)
        if methods:
            top.append("impl PaintRect { " + "\n".join(methods) + " }")
        return "\n".join(top)
    geometry = fixture(OWNER)
    paint = fixture(PIXEL_MATH_OWNER)
    assert not api_contract_violations(OWNER, geometry)
    assert not api_contract_violations(PIXEL_MATH_OWNER, paint)
    assert not api_contract_violations(OWNER, geometry + "\nfn private_helper() {}")
    assert not api_contract_violations(OWNER, geometry.replace("fn aligned_rect", "fn /* comment */ aligned_rect"))
    for name, source in ((OWNER, geometry), (PIXEL_MATH_OWNER, paint)):
        assert api_contract_violations(name, source + "\npub(crate) fn extra() {}")
        assert api_contract_violations(name, source + "\npub(super) const EXTRA:u8=0;")
        assert api_contract_violations(name, source.replace("pub(crate)", "pub", 1))
        assert api_contract_violations(name, source.replace("pub(crate)", "", 1))
        assert api_contract_violations(name, source + "\npub(crate) use other::Extra;")
    assert api_contract_violations(OWNER, geometry.replace("fn aligned_max_rect", "fn changed_method"))
    assert api_contract_violations(OWNER, geometry.replace("FitContent,", "Extra, FitContent,"))
    assert api_contract_violations(OWNER, geometry.replace("->Rect {", "->Vec2 {", 1))
    assert api_contract_violations(PIXEL_MATH_OWNER, paint.replace("PaintRect(Rect)", "PaintRect(pub Rect)"))
    assert api_contract_violations(PIXEL_MATH_OWNER, paint.replace("#[cfg(test)]", ""))
    assert api_contract_violations(PIXEL_MATH_OWNER, paint.replace("impl PaintRect", "impl Other"))
    assert api_contract_violations("screen.rs", "pub(crate) use crate::ui_layout::{geometry::aligned_rect as align};")
    assert api_contract_violations("screen.rs", "pub use crate::ui_layout::pixel_paint::*;")
    assert not api_contract_violations("screen.rs", "use crate::ui_layout::geometry::aligned_rect;")
    assert not api_contract_violations("ui_layout.rs", "pub(crate) mod geometry; pub(crate) mod pixel_paint;")
    assert api_contract_violations("ui_layout.rs", "pub mod geometry; pub(crate) mod pixel_paint;")



# The rest of the shared design system uses a reviewed declaration inventory.
# Do not hash whole source files: changing colors, sizes, or implementation
# bodies must not require an API-contract revision.
DESIGN_API_OWNERS = CORE_COMPONENTS | {
    "typography.rs", "ui_tokens.rs", "scrollbar/mod.rs", "highlight/mod.rs",
    "highlight/native.rs", "ui_layout.rs", "overlay.rs", "text_fit.rs",
    "cell_navigation.rs", "display_format.rs",
}
DESIGN_API_FILES = (
    "egui-design-api-contract.json",
    "egui-design-api-contract-controls.json",
    "egui-design-api-contract-composition.json",
)
DESIGN_API_DIR = Path(__file__).resolve().parent

def load_design_api():
    inventory = {}
    for filename in DESIGN_API_FILES:
        data = json.loads((DESIGN_API_DIR / filename).read_text(encoding="utf-8"))
        if not isinstance(data, dict):
            raise ValueError(f"invalid API inventory: {filename}")
        for owner, rows in data.items():
            if owner in inventory or not isinstance(rows, list) or not all(isinstance(row, str) for row in rows):
                raise ValueError(f"duplicate or invalid API owner: {owner}")
            inventory[owner] = rows
    if set(inventory) != DESIGN_API_OWNERS:
        raise ValueError(f"API inventory coverage mismatch: missing={sorted(DESIGN_API_OWNERS-set(inventory))}, unexpected={sorted(set(inventory)-DESIGN_API_OWNERS)}")
    return inventory

def design_public_surface(source):
    """Extract public declaration tokens, their enclosing type and relevant attributes.

    This is a lexical source contract, not macro expansion or Rust HIR.
    Trait and enum bodies are compared as declarations; public named struct
    fields are separate entries. Constant initializers and ordinary function
    bodies are not part of the contract.
    """
    tokens = api_tokens(production(source))
    stack, scopes, closing = [], [], {}
    for i, token in enumerate(tokens):
        scopes.append(tuple(stack))
        if token == "{":
            stack.append(i)
        elif token == "}":
            if not stack:
                raise ValueError("unbalanced API scope")
            closing[stack.pop()] = i
    if stack:
        raise ValueError("unbalanced API scope")

    def scope(i):
        parents = scopes[i]
        if not parents:
            return "module"
        k = parents[-1]
        j = k - 1
        while j >= 0 and tokens[j] not in ("{", "}", ";"):
            j -= 1
        header = tokens[j+1:k]
        for index, token in enumerate(header):
            if token in ("impl", "struct", "enum", "trait", "union", "mod"):
                return " ".join(header[index:])
        return " ".join(header)

    def end(i, delimiters, angles=False):
        parens = brackets = angle = 0
        for j in range(i, len(tokens)):
            token = tokens[j]
            if not parens and not brackets and not angle and token in delimiters:
                return j
            if token == "(":
                parens += 1
            elif token == ")":
                parens -= 1
            elif token == "[":
                brackets += 1
            elif token == "]":
                brackets -= 1
            if angles and not parens and not brackets:
                if token == "<":
                    angle += 1
                elif token == ">" and angle:
                    angle -= 1
        raise ValueError("unterminated API declaration: " + " ".join(tokens[i:i+25]))

    def attributes(i):
        start, j = i, i-1
        while j >= 0 and tokens[j] == "]":
            depth, k = 1, j-1
            while k >= 0 and depth:
                if tokens[k] == "]":
                    depth += 1
                elif tokens[k] == "[":
                    depth -= 1
                k -= 1
            if k < 0 or tokens[k] != "#":
                break
            if tokens[k+2] in ("cfg", "cfg_attr", "derive", "repr", "non_exhaustive"):
                start = k
            j = k-1
        return start

    rows, i = [], 0
    while i < len(tokens):
        if tokens[i] != "pub":
            i += 1
            continue
        j = i+1
        if tokens[j] == "(":
            j = tokens.index(")", j)+1
        kind = tokens[j]
        if kind in ("const", "async", "unsafe") and tokens[j+1] == "fn":
            kind = "fn"
        if kind == "fn":
            k = end(j, ("{", ";"))+1
        elif kind in ("struct", "enum", "trait", "union", "mod"):
            k = end(j, ("{", ";"))+1
            if kind in ("enum", "trait", "union") and tokens[k-1] == "{":
                k = closing[k-1]+1
        elif kind in ("const", "static"):
            k = end(j, ("=", ";"), angles=True)+1
        elif kind == "use":
            k = tokens.index(";", j)+1
        elif kind == "type":
            k = end(j, (";",))+1
        else:
            k = end(j, (",", "}"), angles=True)
        rows.append(scope(i) + " | " + " ".join(tokens[attributes(i):k]))
        if kind in ("enum", "trait", "union", "use") or (kind == "struct" and tokens[k-1] == ";"):
            i = k
        else:
            i += 1
    return rows

def design_api_violations(name, source, inventory):
    if name not in inventory:
        return []
    expected = Counter(inventory[name])
    actual = Counter(design_public_surface(source))
    findings = []
    for row, count in sorted((actual-expected).items()):
        findings.append((1, f"unapproved public design API ({count}): {row}"))
    for row, count in sorted((expected-actual).items()):
        findings.append((1, f"missing or changed public design API ({count}): {row}"))
    return findings

def design_api_self_test():
    source = """pub(crate) const WIDTH:f32=10.0;
    #[derive(Clone)] pub(crate) struct Card<'a>{ private:bool, pub title:&'a str }
    pub enum Role{Default, Other(bool)}
    pub trait Policy{fn apply(&self);}
    impl<'a> Card<'a>{
        pub(crate) fn render(&self)->bool{ true }
        #[cfg(test)] pub(crate) fn inspect(&self)->bool{false}
    }
    pub(crate) use crate::theme::Role as PublicRole;
    #[cfg(test)] mod tests{ pub fn test_only(){} }
    """
    expected = {"fixture.rs": design_public_surface(source)}
    assert len(expected["fixture.rs"]) == 8
    def check(value):
        return design_api_violations("fixture.rs", value, expected)
    assert not check(source)
    assert not check(source.replace("10.0", "12.0").replace("true }", "false }"))
    assert not check(source.replace("private:bool", "private:u8").replace("fn render", "fn /* comment */ render"))
    assert not check(source + "\nfn internal(){}")
    for changed in (
        source + "\npub(crate) fn extra(){}",
        source.replace("pub(crate) const WIDTH", "pub const WIDTH"),
        source.replace("pub title", "title"),
        source.replace("pub title:&'a str", "pub title:String"),
        source.replace("Other(bool)", "Other(bool), New"),
        source.replace("fn apply(&self);", "fn apply(&self); fn extra(&self);"),
        source.replace("render(&self)->bool", "render(&self)->u8"),
        source.replace("#[cfg(test)] pub(crate) fn inspect", "pub(crate) fn inspect"),
        source.replace("as PublicRole", "as ChangedRole"),
        source.replace("impl<'a> Card<'a>", "impl<'a> Other<'a>"),
    ):
        assert check(changed), changed



# Component-owned design tokens have no public declaration. Shared tokens
# retain their existing owner, with an explicit consumer set checked here.
# Generic composition tokens remain unrestricted. This is a lexical gate,
# not Rust name resolution; it deliberately also detects imports and aliases.
DESIGN_CONSTANT_ACCESS = DESIGN_API_DIR / "egui-design-constant-access.json"

def load_constant_access():
    data = json.loads(DESIGN_CONSTANT_ACCESS.read_text(encoding="utf-8"))
    tokens = data["tokens"]
    if not isinstance(tokens, dict) or len(tokens) != 122:
        raise ValueError("expected all 122 audited shared design constants")
    valid_owners = DESIGN_API_OWNERS | set(API_CONTRACT)
    for name, rule in tokens.items():
        if not re.fullmatch(r"[A-Z][A-Z0-9_]*", name):
            raise ValueError(f"invalid design constant: {name}")
        if rule["owner"] not in valid_owners:
            raise ValueError(f"unknown design owner: {name}")
        if rule["decision"] not in ("internalized", "restricted", "foundation"):
            raise ValueError(f"unknown access decision: {name}")
        if rule["decision"] == "restricted":
            consumers = rule["consumers"]
            if not consumers or len(consumers) != len(set(consumers)) or not set(consumers) <= valid_owners:
                raise ValueError(f"invalid design constant consumers: {name}")
    return tokens

def constant_access_violations(name, source, policy):
    code = production(source)
    findings = []
    restricted = {token: rule for token, rule in policy.items() if rule["decision"] == "restricted"}
    if not restricted:
        return findings
    pattern = re.compile(r"\b(?:" + "|".join(map(re.escape, sorted(restricted))) + r")\b")
    for match in pattern.finditer(code):
        rule = restricted[match.group()]
        if name != rule["owner"] and name not in rule["consumers"]:
            findings.append((code.count("\n", 0, match.start())+1,
                f"design constant {match.group()} restricted to {rule['owner']} and approved components"))
    # No forwarding exports of restricted tokens, even from an approved consumer.
    for export in re.finditer(r"\bpub(?:\s*\([^)]*\))?\s+use\s+[^;]+;", code):
        for match in pattern.finditer(export.group()):
            findings.append((code.count("\n", 0, export.start())+1,
                f"restricted design constant re-export prohibited: {match.group()}"))
    return findings

def constant_access_self_test():
    policy = {
        "PRIVATE_SIZE": {"decision":"internalized","owner":"button/mod.rs"},
        "BUTTON_ONLY_HEIGHT": {"decision":"restricted","owner":"theme.rs","consumers":["button/mod.rs"]},
        "SPACE_8": {"decision":"foundation","owner":"ui_tokens.rs"},
    }
    def check(owner, source):
        return constant_access_violations(owner, source, policy)
    assert not check("theme.rs", "pub const BUTTON_ONLY_HEIGHT:f32=34.0;")
    assert not check("button/mod.rs", "let h=theme::BUTTON_ONLY_HEIGHT;")
    assert check("screen.rs", "let h=theme::BUTTON_ONLY_HEIGHT;")
    assert check("screen.rs", "use crate::theme::BUTTON_ONLY_HEIGHT as H;")
    assert check("button/mod.rs", "pub(crate) use crate::theme::BUTTON_ONLY_HEIGHT as H;")
    assert not check("screen.rs", "let s=\"BUTTON_ONLY_HEIGHT\"; // BUTTON_ONLY_HEIGHT")
    assert not check("screen.rs", "let w=theme::SPACE_8;")
    assert not check("button/mod.rs", "const PRIVATE_SIZE:f32=10.0;")


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
    good = "let paint_rect=crate::ui_layout::pixel_paint::snap_paint_rect(rect,ppp); paint_rect.fill(ui.painter(),0.0,color);"
    assert not violations(PIXEL_OWNER, mixed(good))
    assert not violations(PIXEL_OWNER, mixed(good + "paint_rect.edge_rule(theme::SOFT_DIVIDER_WIDTH,theme::MODAL_MASTER_CONTENT_INSET,ppp,true).fill(ui.painter(),0.0,color);"))
    assert not violations(PIXEL_OWNER, mixed(good + "paint_rect.toolbar_rule(theme::MODAL_HEADER_HEIGHT,theme::SOFT_DIVIDER_WIDTH,theme::MODAL_MASTER_CONTENT_INSET,ppp).fill(ui.painter(),0.0,color);"))
    for bad in (
        "builder.aligned_max_rect(paint_rect);",
        "ui.restrict_shared_clip(paint_rect);",
        "ui.interact(paint_rect,id,sense);",
        "let alias=paint_rect; builder.aligned_max_rect(alias);",
        "ui.shared_allocate_exact_size(paint_rect.size(),sense);",
        "let width=paint_rect.width();",
        "return paint_rect;",
        "let other=crate::ui_layout::pixel_paint::snap_paint_rect(rect,ppp);",
    ):
        assert violations(PIXEL_OWNER, mixed(good + bad)), bad
    for retired in ("modal_master_pixel_aligned_rect", "modal_master_rule_rect",
                    "modal_master_toolbar_rule_rect", "apply_width_policy"):
        assert violations("screen.rs", f"fn {retired}() {{}}")
        assert violations(PIXEL_OWNER, f"{retired}(rect);")
    paint = "fn paint_modal_master_footer_surface(ui:&Ui,rect:Rect) { let ppp=ui.ctx().pixels_per_point(); let rect=crate::ui_layout::pixel_paint::snap_paint_rect(rect,ppp); rect.fill(ui.painter(),0.0,color);"
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
    assert violations(PIXEL_OWNER, "fn layout() { crate::ui_layout::pixel_paint::snap_paint_rect(rect,ppp); }")
    assert not violations("ui_layout.rs", "fn paint_dock_axis_line() { let scale=ui.ctx().pixels_per_point(); let line=pixel_paint::centered_paint_axis_rect(rect,true,width,scale); }")
    assert not violations("ui_layout.rs", "pub(crate) mod pixel_paint;")
    assert not violations(PIXEL_MATH_OWNER, "let x=(value*scale).round()/scale;")
    assert violations(PIXEL_MATH_OWNER, "pub(crate) struct PaintRect(pub Rect);")
    assert violations(PIXEL_MATH_OWNER, "impl From<PaintRect> for Rect { }")
    assert violations(PIXEL_MATH_OWNER, "impl std::ops::Deref for PaintRect { }")
    assert violations(PIXEL_MATH_OWNER, "pub(crate) fn raw(self)->Rect { self.0 }")
    assert not violations(PIXEL_MATH_OWNER, "#[cfg(test)] pub(crate) fn test_rect(self)->Rect { self.0 }")
    assert not violations("kakao_dock.rs", "let to_px=|v:f32|->i32{(v*scale).round() as i32};")

def main():
    self_test()
    api_contract_self_test()
    design_api_self_test()
    constant_access_self_test()
    inventory = load_design_api()
    constant_access = load_constant_access()
    failures = []
    checked = 0
    checked_owners = set()
    required_owners = set(API_CONTRACT) | set(inventory) | {"ui_layout.rs"}
    for path in sorted(ROOT.rglob("*.rs")):
        name = path.relative_to(ROOT).as_posix()
        source = path.read_text(encoding="utf-8-sig")
        if path.name in {"ui_regressions.rs", "ui_test_support.rs", "tests.rs"} or path.stem.endswith("_tests") or re.search(r"#!\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]", source):
            continue
        checked += 1
        if name in required_owners:
            checked_owners.add(name)
        failures.extend(f"{name}:{line}: {rule}" for line, rule in violations(name, source))
        failures.extend(f"{name}:{line}: {rule}" for line, rule in api_contract_violations(name, source))
        failures.extend(f"{name}:{line}: {rule}" for line, rule in design_api_violations(name, source, inventory))
        failures.extend(f"{name}:{line}: {rule}" for line, rule in constant_access_violations(name, source, constant_access))
    for required in sorted(required_owners - checked_owners):
        failures.append(f"{required}:1: required API contract owner missing or excluded from production")
    for finding in failures:
        print(finding, file=sys.stderr)
    print(f"egui design constant access: {dict(Counter(rule['decision'] for rule in constant_access.values()))}")
    print(f"egui design API contract: {len(inventory)+len(API_CONTRACT)} modules, {sum(map(len,inventory.values()))+sum(map(len,API_CONTRACT.values()))} declaration(s)")
    print(f"egui explicit placement ownership: {checked} Rust files, {len(failures)} violation(s)")
    return int(bool(failures))

if __name__ == "__main__":
    sys.exit(main())
