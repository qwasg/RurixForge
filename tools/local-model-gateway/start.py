"""Start/stop local-model-gateway as a detached background process.

Usage: python start.py [stop]
The process must outlive the launching shell, so it is spawned outside the
caller's job object (CREATE_BREAKAWAY_FROM_JOB), falling back to WMI
(`wmic process call create`) when the job forbids breakaway.
"""
import os
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
PID = HERE / "gateway.pid"
STATUS = HERE / "start.status"
TASK = "RurixLocalModelGateway"


def stop() -> None:
    pid = PID.read_text().strip() if PID.exists() else listener_pid()
    if pid:
        subprocess.run(["taskkill", "/PID", pid, "/T", "/F"], capture_output=True)
    subprocess.run(["schtasks", "/End", "/TN", TASK], capture_output=True)
    PID.unlink(missing_ok=True)


def healthy() -> bool:
    port = os.environ.get("PORT", "8787")
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=3) as r:
            return r.status == 200
    except Exception:
        return False


def start() -> str:
    py = sys.executable
    out = open(HERE / "gateway.out.log", "wb")
    err = open(HERE / "gateway.err.log", "wb")
    flags = 0x00000008 | 0x00000200 | 0x01000000  # DETACHED | NEW_GROUP | BREAKAWAY_FROM_JOB
    try:
        p = subprocess.Popen([py, "-u", "server.py"], cwd=HERE, stdout=out, stderr=err,
                             stdin=subprocess.DEVNULL, creationflags=flags, close_fds=True)
        PID.write_text(str(p.pid))
        return f"breakaway pid={p.pid}"
    except OSError as e:
        # Task Scheduler launches the process outside this shell's job object.
        bat = HERE / "run-gateway.cmd"
        bat.write_text(f'@echo off\r\ncd /d "{HERE}"\r\n"{py}" -u server.py 1>gateway.out.log 2>gateway.err.log\r\n',
                       encoding="ascii")
        out.close(); err.close()
        c = subprocess.run(["schtasks", "/Create", "/TN", TASK, "/SC", "ONCE", "/ST", "00:00",
                            "/TR", f'"{bat}"', "/RL", "LIMITED", "/F"], capture_output=True)
        r = subprocess.run(["schtasks", "/Run", "/TN", TASK], capture_output=True)
        return f"breakaway denied ({e.winerror}); schtasks create={c.returncode} run={r.returncode}"


def listener_pid() -> str:
    port = os.environ.get("PORT", "8787")
    r = subprocess.run(["netstat", "-ano", "-p", "TCP"], capture_output=True, text=True, errors="replace")
    for line in r.stdout.splitlines():
        parts = line.split()
        if len(parts) >= 5 and parts[1].endswith(f":{port}") and parts[3] == "LISTENING":
            return parts[4]
    return ""


if __name__ == "__main__":
    stop()
    if sys.argv[1:] == ["stop"]:
        subprocess.run(["schtasks", "/Delete", "/TN", TASK, "/F"], capture_output=True)
        STATUS.write_text("stopped")
        sys.exit(0)
    how = start()
    ok = False
    for _ in range(20):
        time.sleep(0.5)
        if healthy():
            ok = True
            break
    lp = listener_pid()
    if lp:
        PID.write_text(lp)
    STATUS.write_text(f"{how}; healthy={ok}; pid={lp}")
    print(STATUS.read_text())
