"""Run only never-attempted original Cargo executables from a frozen inventory.

Known application-control rejections are not retried or executed from copies.
This collector does not grant any release approval.
"""
from datetime import datetime, timezone
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
from run_native_contracts import PROJECT, REPO, NATIVE, fingerprint, sources

def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()

def write(p, value):
    p.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")

def ref(p):
    return {"path": str(p.relative_to(PROJECT)).replace("\\", "/"), "sha256": sha(p)}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inventory", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    inventory_path, out = args.inventory.resolve(), args.output.resolve()
    inventory = json.loads(inventory_path.read_text(encoding="utf-8"))
    expected_fp = inventory["rulesFingerprint"]
    assert fingerprint() == expected_fp, "Source no longer matches the frozen inventory"
    assert not out.exists(), "Prior receipts must remain untouched"
    candidates = [r for r in inventory["targetLedger"] if r["status"] == "not-attempted"]
    assert sorted(r["target"] for r in candidates) == sorted(inventory["notAttemptedTargets"])
    denied = {r["artifact"]["sha256"] for r in inventory["targetLedger"] if r["target"] in inventory["blockedTargets"]}
    for r in candidates:
        exe = Path(r["originalCargoExecutable"]).resolve()
        assert exe.is_relative_to(NATIVE / "target/release/deps") and exe.suffix == ".exe"
        assert sha(exe) == r["artifact"]["sha256"], r["target"]
    out.mkdir(parents=True)
    before = sources(); write(out / "source-before.json", before)
    fixed = PROJECT / "game/v6/technology-baseline-20260911"
    previous = {p.relative_to(fixed): p.read_bytes() for p in fixed.rglob("*") if p.is_file()}
    for rel, data in previous.items():
        dst = out / "historical-baseline-copy" / rel
        dst.parent.mkdir(parents=True, exist_ok=True); dst.write_bytes(data)
    env = os.environ.copy()
    env["V6_BOT_DIAGNOSTICS"] = str(out / "planned-garrison")
    env["V6_CORE_SIEGE_OUT"] = str(out / "core-siege")
    records = []
    started = datetime.now(timezone.utc).isoformat()
    try:
        for r in candidates:
            assert fingerprint() == expected_fp, "Stop if rules changed"
            exe = Path(r["originalCargoExecutable"])
            assert sha(exe) == r["artifact"]["sha256"], "Executable changed since inventory"
            entry = {"target": r["target"], "originalCargoExecutable": str(exe),
                "artifact": r["artifact"], "startedAtUtc": datetime.now(timezone.utc).isoformat(),
                "status": "not-executed", "passed": None, "failed": None, "ignored": None,
                "automaticRetries": 0}
            records.append(entry)
            if r["artifact"]["sha256"] in denied:
                entry["status"] = "skipped-known-blocked-digest"
                write(out / "live-execution.json", records)
                continue
            command = [str(exe), "--nocapture", "--test-threads=1"]
            entry["command"] = command
            log_path = out / (r["target"] + ".log")
            start = time.monotonic()
            with log_path.open("xb") as log:
                try:
                    code = subprocess.call(command, cwd=REPO, env=env, stdout=log, stderr=subprocess.STDOUT)
                    entry["exitCode"] = code
                    entry["status"] = "executed"
                except OSError as error:
                    entry.update(status="blocked-not-executed" if error.winerror == 4551 else "launch-error-not-executed",
                        winerror=error.winerror, error=str(error), exitCode=None)
                    log.write(str(error).encode("utf-8"))
                    if error.winerror == 4551: denied.add(r["artifact"]["sha256"])
            entry["wallSeconds"] = time.monotonic() - start
            entry["endedAtUtc"] = datetime.now(timezone.utc).isoformat()
            entry["log"] = ref(log_path)
            if entry["status"] == "executed":
                found = re.search(r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;", log_path.read_text(encoding="utf-8", errors="replace"))
                if found:
                    entry.update(passed=int(found[2]), failed=int(found[3]), ignored=int(found[4]))
                else:
                    entry["status"] = "executed-without-test-summary"
            write(out / "live-execution.json", records)
            print(json.dumps({k: entry[k] for k in ("target", "status", "passed", "failed")}), flush=True)
    finally:
        if fixed.exists(): shutil.copytree(fixed, out / "technology-baseline")
        for rel, data in previous.items():
            dst = fixed / rel; dst.parent.mkdir(parents=True, exist_ok=True); dst.write_bytes(data)
        after = sources(); write(out / "source-after.json", after)
        report = {"kind": "previously-unattempted-native-contract-execution", "rulesFingerprint": expected_fp,
            "scope": "Only original, never-attempted executables; known rejected digests are not retried. Partial evidence, not complete suite approval.",
            "inventory": ref(inventory_path), "collector": ref(Path(__file__).resolve()),
            "startedAtUtc": started, "endedAtUtc": datetime.now(timezone.utc).isoformat(),
            "sourceUnchanged": before == after, "currentFingerprint": fingerprint(),
            "historicalBaselinesRestored": all((fixed / rel).read_bytes() == data for rel, data in previous.items()),
            "targets": records, "passedTests": sum(r["passed"] or 0 for r in records),
            "failedTests": sum(r["failed"] or 0 for r in records),
            "notExecuted": [r["target"] for r in records if r["status"] != "executed"],
            "notAttempted": [r["target"] for r in candidates if r["target"] not in {v["target"] for v in records}],
            "finalEligible": False, "automaticRetries": 0}
        write(out / "execution-receipt.json", report)
        print(json.dumps({k: report[k] for k in ("passedTests", "failedTests", "notExecuted", "notAttempted", "sourceUnchanged")}), flush=True)

if __name__ == "__main__":
    main()
