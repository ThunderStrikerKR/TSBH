"""Fail on compiler-confirmed unused code in the egui application itself.

Check both production and unit-test compilation on each CI platform.
Path-imported legacy business modules and integration-test fixture targets
are reported by Cargo but are not confused with egui's own production source.
Force-warn makes local allow attributes unable to silence the selected lints.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

APP = Path(__file__).resolve().parents[1] / "egui-app"
LINTS = ("dead_code", "unused_imports", "unused_variables", "unused_mut",
         "unused_assignments", "unreachable_code")

def app_diagnostic(event):
    if event.get("reason") != "compiler-message":
        return False
    target = event.get("target", {})
    if target.get("name") != "bando-hub-egui" or "bin" not in target.get("kind", []):
        return False
    message = event["message"]
    if (message.get("code") or {}).get("code") not in LINTS:
        return False
    for span in message.get("spans", []):
        if not span.get("is_primary"):
            continue
        path = Path(span["file_name"])
        if not path.is_absolute():
            path = APP / path
        if path.resolve().is_relative_to(APP / "src"):
            return True
    return False

def check(args):
    env = os.environ.copy()
    flags = ["--force-warn=" + lint for lint in LINTS]
    if "CARGO_ENCODED_RUSTFLAGS" in env:
        old = env["CARGO_ENCODED_RUSTFLAGS"]
        env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(([old] if old else []) + flags)
    else:
        env["RUSTFLAGS"] = " ".join([env.get("RUSTFLAGS", ""), *flags]).strip()
    process = subprocess.Popen(
        ["cargo", "check", *args, "--message-format=json"], cwd=APP, env=env,
        stdout=subprocess.PIPE, text=True, encoding="utf-8", errors="replace",
    )
    failures = []
    assert process.stdout is not None
    for line in process.stdout:
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            print(line.rstrip(), file=sys.stderr)
            continue
        if app_diagnostic(event):
            failures.append(event["message"].get("rendered") or event["message"]["message"])
        elif event.get("reason") == "compiler-message":
            msg = event["message"]
            if msg.get("level") == "error":
                print(msg.get("rendered") or msg["message"], file=sys.stderr)
    status = process.wait()
    for diagnostic in failures:
        print(diagnostic, file=sys.stderr)
    print(f"{' '.join(args)}: {len(failures)} unused-code diagnostics, cargo exit {status}")
    return status == 0 and not failures

def self_test():
    def event(path, code="dead_code", name="bando-hub-egui", kind="bin"):
        return {"reason": "compiler-message", "target": {"name": name, "kind": [kind]},
                "message": {"code": {"code": code}, "spans": [{"is_primary": True, "file_name": path}]}}
    assert app_diagnostic(event("src/theme.rs"))
    assert app_diagnostic(event(str(APP / "src" / "button" / "mod.rs"), "unused_imports"))
    assert not app_diagnostic(event("src/../../src-tauri/src/business_core/rules/mod.rs"))
    assert not app_diagnostic(event("src/theme.rs", code="deprecated"))
    assert not app_diagnostic(event("src/theme.rs", name="bando-data-access"))
    assert not app_diagnostic(event("src/theme.rs", kind="test"))

if __name__ == "__main__":
    self_test()
    if "--self-test" not in sys.argv:
        production = check(["--bin", "bando-hub-egui"])
        tests = check(["--tests"])
        sys.exit(0 if production and tests else 1)
