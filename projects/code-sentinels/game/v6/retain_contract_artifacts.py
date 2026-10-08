"""Retain executed contract binaries before a later build reuses Cargo paths."""
from __future__ import annotations
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import shutil

def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    args = parser.parse_args()
    original = args.report.resolve()
    report = json.loads(original.read_text(encoding="utf-8"))
    destination = original.parent / "executed-test-artifacts"
    result = original.parent / "contract-execution-retained.json"
    if destination.exists() or result.exists():
        raise SystemExit("Existing retained evidence preserved; no overwrite.")
    if report.get("unexecutedTargets") or report.get("missingTargets"):
        raise SystemExit("Cannot retain an incomplete invocation as executed coverage.")
    planned = []
    for target in report["targets"]:
        artifact = target["artifact"]
        src = Path(artifact["path"])
        if not src.is_absolute() or sha(src) != artifact["sha256"]:
            raise SystemExit(f"Executed artifact is unavailable or changed: {src}")
        dst = destination / (src.stem + "-" + artifact["sha256"][:16] + src.suffix)
        planned.append((target, src, dst))
    destination.mkdir()
    retained = []
    for target, src, dst in planned:
        if not dst.exists():
            shutil.copy2(src, dst)
        if sha(dst) != target["artifact"]["sha256"]:
            raise SystemExit(f"Retained artifact failed readback: {dst}")
        target["artifact"]["originalCargoBuildPath"] = str(src)
        target["artifact"]["path"] = str(dst)
        retained.append({"target": target["target"], **target["artifact"]})
    report["artifactRetention"] = {
        "recordedAtUtc": datetime.now(timezone.utc).isoformat(),
        "scope": "Byte-identical copies of artifacts observed by the existing invocation. No new test execution and no changed outcome.",
        "originalReport": {"path": str(original), "sha256": sha(original)},
        "collector": {"path": str(Path(__file__).resolve()), "sha256": sha(Path(__file__))},
        "artifacts": retained,
    }
    result.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({"path": str(result), "sha256": sha(result), "retainedTargets": len(retained)}))

if __name__ == "__main__":
    main()
