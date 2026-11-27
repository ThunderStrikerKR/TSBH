"""Resolve candidate OCR wheels, never execute or approve them."""
import hashlib
import json
import os
import platform
import re
import subprocess
import sys
from pathlib import Path
from urllib.parse import urlsplit
from urllib.request import urlopen
from zipfile import ZipFile
from email.parser import BytesParser

def run(*args):
    subprocess.run([sys.executable, "-m", "pip", "--isolated",
                    "--disable-pip-version-check", *args], check=True,
                   env={**os.environ, "PIP_CONFIG_FILE": os.devnull})

def canonical(name):
    return re.sub(r"[-_.]+", "-", name).lower()

def main():
    if sys.platform != "win32" or sys.version_info[:2] != (3, 11) or platform.machine().lower() not in {"amd64", "x86_64"}:
        raise SystemExit("Windows x64 CPython 3.11 is required.")
    root = Path(__file__).resolve().parents[1]
    output = Path(sys.argv[1]).resolve()
    output.mkdir(parents=True, exist_ok=False)
    wheels = output / "wheels"
    wheels.mkdir()
    report = output / "resolution.json"
    run("install", "--dry-run", "--ignore-installed", "--only-binary=:all:",
        "--no-cache-dir", "--index-url", "https://pypi.org/simple",
        "--report", str(report), "-r", str(root / "security/ocr/requirements.in"))
    selected = {}
    for entry in json.loads(report.read_text(encoding="utf-8"))["install"]:
        name = canonical(entry["metadata"]["name"])
        version = entry["metadata"]["version"]
        info = entry["download_info"]
        url = urlsplit(info["url"])
        digest = info.get("archive_info", {}).get("hashes", {}).get("sha256", "")
        if (entry.get("is_direct") or url.scheme != "https" or url.hostname != "files.pythonhosted.org"
                or url.username or url.password or url.query or url.fragment
                or not url.path.endswith(".whl") or not re.fullmatch(r"[a-f0-9]{64}", digest)
                or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.+-]*", version)
                or name in selected):
            raise ValueError(f"Unapproved wheel source or resolution: {name}")
        selected[name] = (version, digest)
    if not {"paddleocr", "paddlepaddle"} <= selected.keys():
        raise ValueError("Missing OCR roots")
    lock = output / "requirements.lock"
    lock.write_text("# Candidate: Windows x64 CPython 3.11; independent approval required.\n"
                    + "".join(f"{name}=={version} --hash=sha256:{digest}\n"
                              for name, (version, digest) in sorted(selected.items())),
                    encoding="utf-8")
    run("download", "--only-binary=:all:", "--require-hashes", "--no-cache-dir",
        "--index-url", "https://pypi.org/simple", "--dest", str(wheels), "-r", str(lock))
    seen = set()
    inventory = []
    for wheel in sorted(wheels.iterdir()):
        if wheel.suffix != ".whl" or not wheel.is_file():
            raise ValueError("Non-wheel download")
        with ZipFile(wheel) as archive:
            metadata = [x for x in archive.namelist() if x.endswith(".dist-info/METADATA")]
            if len(metadata) != 1 or archive.getinfo(metadata[0]).file_size > 4 * 1024 * 1024:
                raise ValueError("Invalid wheel metadata")
            msg = BytesParser().parsebytes(archive.read(metadata[0]))
        name = canonical(msg["Name"])
        version = msg["Version"]
        with wheel.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        if name in seen or selected.get(name) != (version, digest):
            raise ValueError(f"Wheel does not match resolution: {name}")
        seen.add(name)
        inventory.append({"name": name, "version": version, "file": wheel.name, "sha256": digest})
    if seen != selected.keys():
        raise ValueError("Incomplete wheelhouse")
    run("install", "--dry-run", "--ignore-installed", "--no-index", "--only-binary=:all:",
        "--find-links", str(wheels), "--require-hashes", "-r", str(lock))
    (output / "wheel-inventory.json").write_text(json.dumps(inventory, indent=2) + "\n", encoding="utf-8")
    print(f"Resolved and hash-verified {len(seen)} candidate wheels; no OCR installation or approval.")
    print("OCR_LOCK_BEGIN")
    print(lock.read_text(encoding="utf-8"), end="")
    print("OCR_LOCK_END")

if __name__ == "__main__":
    main()
