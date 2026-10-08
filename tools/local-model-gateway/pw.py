"""Run one playwright-cli command and save its output to pw.out (this shell swallows stdout).

Usage: python pw.py <playwright-cli args...>
The literal argument __TOKEN__ is replaced with LOCAL_API_KEY from .env, so the
token never appears on the calling command line.
"""
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def token() -> str:
    for line in (HERE / ".env").read_text(encoding="utf-8").splitlines():
        if line.startswith("LOCAL_API_KEY="):
            return line.split("=", 1)[1].strip().strip('"').strip("'")
    return ""


args = []
for a in sys.argv[1:]:
    if a == "__TOKEN__":
        args.append(token())
    else:
        args.append(a)
r = subprocess.run([shutil.which("playwright-cli"), *args], capture_output=True, cwd=HERE, timeout=600)
out = (r.stdout + r.stderr).decode("utf-8", "replace")
tok = token()
if tok:
    out = out.replace(tok, "***")
(HERE / "pw.out").write_text(f"exit={r.returncode}\n{out}", encoding="utf-8")
