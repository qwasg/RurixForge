"""Run the native contracts once, retaining real output and pre-existing reports.

This records execution, never approves a release or retries an OS refusal.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time

PROJECT = Path(__file__).resolve().parents[2]
REPO = PROJECT.parents[1]
NATIVE = PROJECT / "native-v6"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")


def sources():
    paths = [*NATIVE.joinpath("src").rglob("*.rs"), *NATIVE.joinpath("tests").rglob("*.rs")]
    paths += [NATIVE / name for name in ("Cargo.toml", "Cargo.lock", "build.rs")]
    return [{"path": str(p.relative_to(PROJECT)).replace("\\", "/"),
             "sha256": sha(p), "bytes": p.stat().st_size}
            for p in sorted(paths) if p.is_file()]


def fingerprint():
    paths = list(NATIVE.joinpath("src").rglob("*.rs"))
    paths += [NATIVE / name for name in ("Cargo.toml", "Cargo.lock", "build.rs")]
    digest = hashlib.sha256(b"code-sentinels-native-rules-v1\0")
    for p in sorted(paths, key=lambda p: p.relative_to(NATIVE).as_posix()):
        name, data = p.relative_to(NATIVE).as_posix().encode(), p.read_bytes()
        digest.update(len(name).to_bytes(8, "little")); digest.update(name)
        digest.update(len(data).to_bytes(8, "little")); digest.update(data)
    return digest.hexdigest()


def parse(log):
    records, current = [], None
    for number, line in enumerate(log.splitlines(), 1):
        running = re.search(r"Running\s+(.+?)\s+\((.+\.exe)\)", line)
        if running:
            artifact = Path(running[2])
            if not artifact.is_absolute():
                artifact = REPO / artifact
            current = {"target": running[1].replace("\\", "/"), "logLine": number,
                       "status": "unexecuted", "artifact": {"path": str(artifact)}}
            if artifact.is_file():
                current["artifact"].update(sha256=sha(artifact), bytes=artifact.stat().st_size)
            records.append(current)
        result = re.search(r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;", line)
        if result and current:
            current.update(status="executed", passed=int(result[2]), failed=int(result[3]),
                           ignored=int(result[4]), resultLogLine=number)
            current = None
        if current and "os error 4551" in line:
            current["refusal"] = "Windows application control, os error 4551; not executed"
    return records


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--output", required=True, type=Path)
    p.add_argument("--expected-fingerprint", required=True)
    p.add_argument("--profile", choices=["release", "debug"], default="release")
    p.add_argument("--integration-only", action="store_true")
    args = p.parse_args()
    if fingerprint() != args.expected_fingerprint.lower():
        raise SystemExit("Current Game source differs from the requested rules fingerprint.")
    output = args.output.resolve()
    if output.exists():
        raise SystemExit("Existing evidence preserved; select a new output directory.")
    output.mkdir(parents=True)
    before = sources(); write(output / "source-before.json", before)
    # This historical test has a fixed output location. Preserve bytes before it
    # runs, archive this run's actual outputs, and restore the prior bytes finally.
    fixed = PROJECT / "game/v6/technology-baseline-20260911"
    previous = {q.relative_to(fixed): q.read_bytes() for q in fixed.rglob("*") if q.is_file()}
    write(output / "historical-baseline-files.json", [
        {"path": str(q), "sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)}
        for q, data in sorted(previous.items())])
    for rel, data in previous.items():
        dest = output / "historical-baseline-copy" / rel
        dest.parent.mkdir(parents=True, exist_ok=True); dest.write_bytes(data)
    env = os.environ.copy()
    env["V6_BOT_DIAGNOSTICS"] = str(output / "planned-garrison")
    env["V6_CORE_SIEGE_OUT"] = str(output / "core-siege")
    command = ["cargo", "test", "--manifest-path", str(NATIVE / "Cargo.toml"), "--no-fail-fast"]
    if args.profile == "release": command.append("--release")
    expected = []
    if not args.integration_only:
        command.append("--lib"); expected.append("unittests src/lib.rs")
    for test in sorted(NATIVE.joinpath("tests").glob("*.rs")):
        command += ["--test", test.stem]; expected.append(f"tests/{test.name}")
    command += ["--", "--nocapture", "--test-threads=1"]
    started = datetime.now(timezone.utc).isoformat(); start = time.monotonic()
    write(output / "invocation.json", {"command": command, "cwd": str(REPO), "profile": args.profile,
          "startedAtUtc": started, "rulesFingerprint": args.expected_fingerprint,
          "expectedTargets": expected, "automaticRetries": 0})
    code = None
    try:
        with (output / "cargo-tests.log").open("wb") as log:
            code = subprocess.call(command, cwd=REPO, env=env, stdout=log, stderr=subprocess.STDOUT)
    finally:
        if fixed.exists(): shutil.copytree(fixed, output / "technology-baseline", dirs_exist_ok=False)
        for rel, data in previous.items():
            dest = fixed / rel; dest.parent.mkdir(parents=True, exist_ok=True); dest.write_bytes(data)
        after = sources(); write(output / "source-after.json", after)
        records = parse((output / "cargo-tests.log").read_text(encoding="utf-8-sig", errors="replace"))
        seen = {record["target"] for record in records}
        report = {"kind": "native-contract-execution", "finalEligible": False,
                  "rulesFingerprint": args.expected_fingerprint, "profile": args.profile,
                  "startedAtUtc": started, "endedAtUtc": datetime.now(timezone.utc).isoformat(),
                  "wallSeconds": time.monotonic() - start, "exitCode": code,
                  "sourceUnchanged": before == after, "currentFingerprint": fingerprint(),
                  "historicalBaselinesRestored": all((fixed / rel).read_bytes() == data for rel, data in previous.items()),
                  "targets": records, "missingTargets": sorted(set(expected) - seen),
                  "passedTests": sum(r.get("passed", 0) for r in records),
                  "failedTests": sum(r.get("failed", 0) for r in records),
                  "ignoredTests": sum(r.get("ignored", 0) for r in records),
                  "unexecutedTargets": [r["target"] for r in records if r["status"] != "executed"],
                  "automaticRetries": 0,
                  "evidence": [{"path": str(q.relative_to(PROJECT)), "sha256": sha(q)}
                      for q in (output / "invocation.json", output / "source-before.json", output / "source-after.json", output / "cargo-tests.log")]}
        write(output / "contract-execution.json", report)
        print(json.dumps({k: report[k] for k in ("exitCode", "passedTests", "failedTests", "ignoredTests", "unexecutedTargets", "missingTargets", "sourceUnchanged")}, ensure_ascii=False))
    if code or not report["sourceUnchanged"] or report["missingTargets"] or report["unexecutedTargets"]:
        raise SystemExit(code or 1)


if __name__ == "__main__":
    main()
