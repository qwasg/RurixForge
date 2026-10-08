"""Drive the playground page via playwright-cli: select each model, click 发送, record the status.

Assumes the page is already open, token filled and model list loaded (see pw.py).
Usage: python ui_test.py model1 model2 ...   -> writes ui_report.json
"""
import json
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLI = shutil.which("playwright-cli")


def pw(*args: str) -> str:
    r = subprocess.run([CLI, *args], capture_output=True, cwd=HERE, timeout=120)
    return (r.stdout + r.stderr).decode("utf-8", "replace")


def status() -> str:
    snap = pw("snapshot")
    m = re.search(r'- status \[ref=\w+\]: "?(.*?)"?$', snap, re.M)
    out = re.search(r'- heading "输出".*?\n\s+- (?:generic|img).*?: ?"?(.*?)"?$', snap, re.M | re.S)
    return (m.group(1) if m else ""), (out.group(1)[:200] if out else "")


results = []
for model in sys.argv[1:]:
    pw("select", "e11", model)
    pw("click", "e16")
    t0 = time.time()
    st, out = "", ""
    while time.time() - t0 < 180:
        time.sleep(3)
        st, out = status()
        if st and "中…" not in st:
            break
    results.append({"model": model, "status": st, "output": out, "wall_s": round(time.time() - t0, 1)})
    (HERE / "ui_report.json").write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")
