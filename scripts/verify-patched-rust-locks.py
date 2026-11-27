"""Reject known vulnerable dependency versions in the Cargo-resolved files."""
import pathlib
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]

def version(value):
    parts = value.split(".")
    if len(parts) != 3 or not all(p.isdecimal() for p in parts):
        raise ValueError(f"Unexpected non-stable version: {value}")
    return tuple(map(int, parts))

def check(path, rules):
    with path.open("rb") as stream:
        packages = tomllib.load(stream)["package"]
    for name, lower in rules.items():
        found = [p["version"] for p in packages if p["name"] == name]
        if not found or any(version(v) < lower for v in found):
            raise SystemExit(f"{path.name} ({path.parent.name}): {name} {found}; minimum {lower}")
        print(f"{path.parent.name}: {name} {', '.join(found)}")

for directory in ("egui-app", "crates/bando-data-access", "src-tauri"):
    rules = {"rusqlite": (0, 41, 1), "libsqlite3-sys": (0, 39, 0)}
    if directory == "egui-app":
        rules.update({"zip": (9, 0, 5), "rust_xlsxwriter": (0, 104, 0)})
    check(ROOT / directory / "Cargo.lock", rules)
