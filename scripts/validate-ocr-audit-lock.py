"""Validate a non-secret OCR dependency inventory without installing packages."""
import re
import sys
from pathlib import Path

REQUIREMENT = re.compile(
    r"([A-Za-z0-9][A-Za-z0-9_.-]*)==([A-Za-z0-9][A-Za-z0-9_.+-]*)"
    r"(?:\s+--hash=sha256:[A-Fa-f0-9]{64})+"
)

def validate(path):
    packages = set()
    for number, raw in enumerate(path.read_text(encoding="utf-8-sig").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        match = REQUIREMENT.fullmatch(line)
        if not match:
            raise ValueError(f"Invalid exact, hashed requirement at line {number}")
        name = re.sub(r"[-_.]+", "-", match[1]).lower()
        if name in packages:
            raise ValueError(f"Duplicate dependency at line {number}")
        packages.add(name)
    if not {"paddleocr", "paddlepaddle"} <= packages:
        raise ValueError("OCR inventory must include both PaddleOCR and PaddlePaddle")
    return len(packages)

if __name__ == "__main__":
    try:
        count = validate(Path(sys.argv[1]))
    except (OSError, ValueError, IndexError) as exc:
        print(f"BLOCKED: {exc}", file=sys.stderr)
        sys.exit(1)
    print(f"Validated {count} exact hashed package entries; completeness requires independent wheelhouse review.")
