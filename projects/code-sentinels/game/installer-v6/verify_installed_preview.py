"""Verify actual installed bytes; do not infer success from the compiler log."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


ap = argparse.ArgumentParser()
ap.add_argument("--work", type=Path, required=True)
ap.add_argument("--install-root", type=Path, required=True)
args = ap.parse_args()
work, root = args.work.resolve(strict=True), args.install_root.resolve(strict=True)
manifest_path = work / "installed-files.json"
stage_receipt = json.loads((work / "staging-receipt.json").read_text(encoding="utf-8-sig"))
assert digest(manifest_path) == stage_receipt["installedFilesManifestSha256"]
items = json.loads(manifest_path.read_text(encoding="utf-8-sig"))["files"]
failures = []
for item in items:
    file = root / item["path"]
    if not file.resolve().is_relative_to(root):
        failures.append({"path": item["path"], "error": "escapes-root"})
    elif not file.is_file():
        failures.append({"path": item["path"], "error": "missing"})
    elif file.stat().st_size != item["bytes"] or digest(file) != item["sha256"]:
        failures.append({"path": item["path"], "error": "bytes-or-hash-mismatch"})
expected = {item["path"] for item in items}
extra = sorted(p.relative_to(root).as_posix() for p in root.rglob("*") if p.is_file() and p.relative_to(root).as_posix() not in expected)
unexpected = [name for name in extra if "/" in name or not name.lower().startswith("unins000.")]
passed = not failures and not unexpected
receipt = {"checkedAtUtc": datetime.now(timezone.utc).isoformat(), "status": "installed-bytes-verified" if passed else "installed-bytes-failed",
           "installRoot": str(root), "checkedFiles": len(items), "failures": failures,
           "installerGeneratedFiles": extra, "unexpectedFiles": unexpected, "passed": passed,
           "runtimeStartedByThisCheck": False, "fullGameAcceptanceClaimed": False}
(work / "acceptance" / "installed-files-verification.json").write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(json.dumps(receipt, ensure_ascii=False))
raise SystemExit(0 if passed else 1)
