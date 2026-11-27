"""Reject prohibited wording in version-controlled runtime source and resources.

This is deliberately a conservative static text gate, not a runtime renderer.
Exact, counted exceptions preserve old persisted-event aliases and existing
regression fixtures; whole files and inline test modules are never exempt.
"""
from collections import Counter
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
EXCEPTIONS = ROOT / "scripts/ui-terminology-legacy-exceptions.json"
ROOTS = (
    "egui-app/src", "crates", "src-tauri/src", "src-tauri/resources",
    "src", "public", "data", "mobile",
)
EXTENSIONS = {
    ".rs", ".json", ".jsonc", ".toml", ".sql", ".py", ".ts", ".tsx",
    ".js", ".jsx", ".mjs", ".cjs", ".html", ".css", ".scss", ".txt",
    ".yaml", ".yml", ".xml", ".svg", ".csv", ".properties", ".ftl",
}
FORBIDDEN = ("직원", "실제")
ESCAPE = re.compile(r"\\u(?:\{([0-9a-fA-F_]{1,8})\}|([0-9a-fA-F]{4}))")


def normalize(line):
    def decode(match):
        try:
            return chr(int((match.group(1) or match.group(2)).replace("_", ""), 16))
        except ValueError:
            return match.group(0)
    return ESCAPE.sub(decode, line)


def violations(path, source, allowed):
    errors = []
    for number, line in enumerate(source.splitlines(), 1):
        if not any(word in normalize(line) for word in FORBIDDEN):
            continue
        key = (path, line.strip())
        if allowed[key] > 0:
            allowed[key] -= 1
        else:
            errors.append(f"{path}:{number}: prohibited wording: {line.strip()}")
    return errors


def load_exceptions():
    rows = json.loads(EXCEPTIONS.read_text(encoding="utf-8"))
    allowed = Counter()
    for row in rows:
        key = (row["path"], row["line"])
        if not row["reason"].strip() or not any(word in normalize(row["line"]) for word in FORBIDDEN):
            raise ValueError(f"Invalid terminology exception: {key!r}")
        if allowed[key]:
            raise ValueError(f"Duplicate terminology exception: {key!r}")
        allowed[key] = 1
    return allowed


def self_test():
    path = "egui-app/src/example.rs"
    legacy = 'match old { "직원" => "작업자 미지정", _ => old }'
    allowed = Counter({(path, legacy): 1})
    assert not violations(path, legacy, allowed)
    assert violations(path, legacy, allowed), "duplicated legacy line must fail"
    assert violations(path, 'ui.label("직원 확인");', Counter())
    assert violations(path, 'ui.label("실제 입금");', Counter())
    assert not violations(path, 'ui.label("출고 운임");', Counter())
    assert violations(path, r'ui.label("\u{C2E4}\u{C81C}");', Counter())
    assert violations(path, r'ui.label("\u{C9C1}\u{C6D0}");', Counter())
    assert violations(path, r'{"label":"\uc9c1\uc6d0"}', Counter())
    assert violations("other.rs", legacy, Counter({(path, legacy): 1}))
    assert not violations(path, 'ui.label("작업자 확인");', Counter())
    print("Terminology gate self-test passed.")


def main():
    self_test()
    if "--self-test" in sys.argv:
        return 0
    allowed = load_exceptions()
    tracked = subprocess.check_output(
        ["git", "ls-files", "-z", "--", *ROOTS], cwd=ROOT
    ).decode("utf-8").split("\0")
    paths = sorted({p for p in tracked if p and Path(p).suffix.lower() in EXTENSIONS})
    if not paths:
        raise RuntimeError("No runtime source files found; refusing to pass.")
    errors = []
    for path in paths:
        try:
            source = (ROOT / path).read_text(encoding="utf-8-sig")
        except (OSError, UnicodeError) as exc:
            errors.append(f"{path}: cannot inspect: {exc}")
            continue
        errors.extend(violations(path, source, allowed))
    for (path, line), count in allowed.items():
        if count > 0:
            errors.append(f"{path}: unused exception (remove/review): {line}")
    for error in errors:
        print(error, file=sys.stderr)
    print(f"Scanned {len(paths)} tracked source/resource files; {len(errors)} violations.")
    return int(bool(errors))


if __name__ == "__main__":
    sys.exit(main())
