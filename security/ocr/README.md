# OCR dependency audit inventory

`requirements.lock` is intentionally absent until a genuine, complete, Windows x64 CPython 3.11 wheelhouse inventory is reviewed. Do not create a dummy lockfile to make CI green.

Record each direct and transitive dependency as one line: exact package name, ==version, and one or more --hash=sha256: options with verified wheel hashes. Include PaddleOCR and PaddlePaddle. No URLs, credentials, local paths, pip options or customer data. The CI validates the format and audits the named versions without installing OCR. The release-time approved runtime metadata audit remains mandatory because this inventory alone cannot establish wheelhouse completeness, Python interpreter security, or artifact integrity.

Use the identical inventory in the independently approved offline OCR input directory.
