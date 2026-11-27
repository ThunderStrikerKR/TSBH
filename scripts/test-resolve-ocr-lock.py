"""Synthetic tests; no dependency installation, network or real OCR data."""
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest
from zipfile import ZipFile

spec = importlib.util.spec_from_file_location("resolver", Path(__file__).with_name("resolve-ocr-lock.py"))
resolver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(resolver)

class WheelIdentityTests(unittest.TestCase):
    def check_wheel(self, entries, expected_error=None, bad_hash=False):
        with tempfile.TemporaryDirectory() as temp:
            wheel = Path(temp) / "demo_pkg-1.2.3-py3-none-any.whl"
            with ZipFile(wheel, "w") as archive:
                for name, value in entries:
                    archive.writestr(name, value)
            digest = hashlib.sha256(wheel.read_bytes()).hexdigest()
            selected = {"demo-pkg": ("1.2.3", "0" * 64 if bad_hash else digest)}
            if expected_error:
                with self.assertRaisesRegex(ValueError, expected_error):
                    resolver.verify_wheel(wheel, selected)
            else:
                self.assertEqual(resolver.verify_wheel(wheel, selected), ("demo-pkg", "1.2.3", digest))

    def test_nested_vendor_metadata_is_not_distribution_identity(self):
        self.check_wheel([
            ("demo_pkg-1.2.3.dist-info/METADATA", "Name: demo-pkg\nVersion: 1.2.3\n"),
            ("demo_pkg/vendor/other-9.dist-info/METADATA", "Name: other\nVersion: 9\n"),
        ])

    def test_duplicate_root_metadata_rejected(self):
        self.check_wheel([
            ("demo_pkg-1.2.3.dist-info/METADATA", "Name: demo-pkg\nVersion: 1.2.3\n"),
            ("other-9.dist-info/METADATA", "Name: other\nVersion: 9\n"),
        ], "expected one top-level")

    def test_mismatched_identity_rejected(self):
        self.check_wheel([("demo_pkg-1.2.3.dist-info/METADATA", "Name: other\nVersion: 1.2.3\n")], "identity differs")

    def test_hash_mismatch_rejected_before_metadata(self):
        self.check_wheel([], "hash differs", bad_hash=True)

    def test_oversized_metadata_rejected(self):
        self.check_wheel([("demo_pkg-1.2.3.dist-info/METADATA", b"x" * (resolver.MAX_METADATA_BYTES + 1))], "exceeds")

if __name__ == "__main__":
    unittest.main()
