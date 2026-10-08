"""Run the official compiler and retain each actual attempt and artifact hash."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import sys


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


ap = argparse.ArgumentParser()
ap.add_argument("--work", type=Path, required=True)
ap.add_argument("--iscc", type=Path, required=True)
ap.add_argument("--chinese", type=Path, required=True)
ap.add_argument("--attempt", required=True)
args = ap.parse_args()
work = args.work.resolve(strict=True)
attempt = work / ("compile-" + args.attempt)
attempt.mkdir(exist_ok=False)
script = work / "installer.iss"
command = [str(args.iscc.resolve(strict=True)), "/Qp", "/DChineseMessages=" + str(args.chinese.resolve(strict=True)), str(script)]
started = datetime.now(timezone.utc).isoformat()
save(attempt / "started.json", {"startedAtUtc": started, "command": command, "scriptSha256": sha(script), "compilerSha256": sha(args.iscc), "chineseMessagesSha256": sha(args.chinese)})
with (attempt / "compiler.log").open("wb") as log:
    child = subprocess.Popen(command, cwd=work, stdout=log, stderr=subprocess.STDOUT,
                             creationflags=subprocess.CREATE_NO_WINDOW | subprocess.BELOW_NORMAL_PRIORITY_CLASS)
    save(attempt / "process.json", {"pid": child.pid, "startedAtUtc": started})
    code = child.wait()
outputs = []
for file in sorted((work / "output").glob("*.exe")):
    outputs.append({"path": str(file), "bytes": file.stat().st_size, "sha256": sha(file)})
receipt = {"startedAtUtc": started, "completedAtUtc": datetime.now(timezone.utc).isoformat(), "exitCode": code,
           "compilerSha256": sha(args.iscc), "scriptSha256": sha(script), "compilerLogSha256": sha(attempt / "compiler.log"),
           "installedFilesManifestSha256": sha(work / "installed-files.json"), "outputs": outputs,
           "installerExecutionAttempted": False, "signed": False, "finalGameAcceptanceClaimed": False}
save(attempt / "receipt.json", receipt)
print(json.dumps(receipt, ensure_ascii=False))
if code:
    print((attempt / "compiler.log").read_text(encoding="utf-8-sig", errors="replace")[-5000:])
sys.exit(code)
