#!/usr/bin/env python3
"""Opt-in live Codex collaboration acceptance; uses existing CLI authentication.

Windows example (build the daemon separately first):
  py -3 tools/e2e/collaboration-codex-smoke.py --agentd target/debug/forge-agentd.exe --codex C:/path/to/codex.exe

No downloads or project edits. Each run creates a C: temporary workspace, copies
the daemon, and retains its isolated data/evidence. --resume-root reuses only a
workspace created by this script. All owned process trees stop in finally.
This is a live model test and consumes the selected account's normal usage.
"""

import argparse
import collections
import concurrent.futures
import ctypes
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid


SCHEMA = "rurixforge.collaboration.codex-smoke.v1"
ALLOWED_TOOLS = {
    "task", "agent_list", "send_message", "plan_write", "team_get", "team_create",
    "team_member_spawn", "team_task_create", "team_task_update", "team_task_list",
    "team_task_claim", "team_task_report", "team_control",
}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def save(path, value, exclusive=False):
    with path.open("x" if exclusive else "w", encoding="utf-8") as file:
        json.dump(value, file, ensure_ascii=False, indent=2)


class ProcessTree:
    """A Windows Job Object owns only the daemon and descendants it creates."""

    def __init__(self, process):
        from ctypes import wintypes as wt

        class BasicLimits(ctypes.Structure):
            _fields_ = [("process_time", ctypes.c_int64), ("job_time", ctypes.c_int64),
                        ("flags", wt.DWORD), ("minimum", ctypes.c_size_t),
                        ("maximum", ctypes.c_size_t), ("active", wt.DWORD),
                        ("affinity", ctypes.c_size_t), ("priority", wt.DWORD),
                        ("scheduling", wt.DWORD)]

        class IoCounters(ctypes.Structure):
            _fields_ = [(name, ctypes.c_uint64) for name in
                        ("reads", "writes", "other", "read_bytes", "write_bytes", "other_bytes")]

        class ExtendedLimits(ctypes.Structure):
            _fields_ = [("basic", BasicLimits), ("io", IoCounters),
                        ("process_memory", ctypes.c_size_t), ("job_memory", ctypes.c_size_t),
                        ("peak_process", ctypes.c_size_t), ("peak_job", ctypes.c_size_t)]

        self.api = ctypes.WinDLL("kernel32", use_last_error=True)
        self.api.CreateJobObjectW.argtypes = [ctypes.c_void_p, wt.LPCWSTR]
        self.api.CreateJobObjectW.restype = wt.HANDLE
        self.api.SetInformationJobObject.argtypes = [wt.HANDLE, ctypes.c_int, ctypes.c_void_p, wt.DWORD]
        self.api.AssignProcessToJobObject.argtypes = [wt.HANDLE, wt.HANDLE]
        self.api.CloseHandle.argtypes = [wt.HANDLE]
        self.handle = self.api.CreateJobObjectW(None, None)
        limits = ExtendedLimits()
        limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        try:
            require(self.handle, "Cannot create an isolated process job")
            require(self.api.SetInformationJobObject(self.handle, 9, ctypes.byref(limits), ctypes.sizeof(limits)),
                    "Cannot configure isolated process cleanup")
            require(self.api.AssignProcessToJobObject(self.handle, int(process._handle)),
                    "Cannot own the daemon process tree")
            self.resume(process.pid)
        except BaseException:
            process.terminate()
            process.wait(timeout=10)
            self.close()
            raise

    def resume(self, process_id):
        """Resume the sole thread only after the suspended child joins our job."""
        from ctypes import wintypes as wt

        class ThreadEntry(ctypes.Structure):
            _fields_ = [("size", wt.DWORD), ("usage", wt.DWORD), ("id", wt.DWORD),
                        ("owner", wt.DWORD), ("base_priority", wt.LONG),
                        ("delta_priority", wt.LONG), ("flags", wt.DWORD)]

        self.api.CreateToolhelp32Snapshot.argtypes = [wt.DWORD, wt.DWORD]
        self.api.CreateToolhelp32Snapshot.restype = wt.HANDLE
        self.api.Thread32First.argtypes = [wt.HANDLE, ctypes.POINTER(ThreadEntry)]
        self.api.Thread32Next.argtypes = [wt.HANDLE, ctypes.POINTER(ThreadEntry)]
        self.api.OpenThread.argtypes = [wt.DWORD, wt.BOOL, wt.DWORD]
        self.api.OpenThread.restype = wt.HANDLE
        self.api.ResumeThread.argtypes = [wt.HANDLE]
        self.api.ResumeThread.restype = wt.DWORD
        snapshot = self.api.CreateToolhelp32Snapshot(0x4, 0)  # TH32CS_SNAPTHREAD
        require(snapshot not in (None, ctypes.c_void_p(-1).value), "Cannot inspect owned suspended process")
        try:
            entry = ThreadEntry()
            entry.size = ctypes.sizeof(entry)
            present = self.api.Thread32First(snapshot, ctypes.byref(entry))
            while present:
                if entry.owner == process_id:
                    thread = self.api.OpenThread(0x2, False, entry.id)  # THREAD_SUSPEND_RESUME
                    require(thread, "Cannot open owned suspended thread")
                    try:
                        require(self.api.ResumeThread(thread) != 0xFFFFFFFF, "Cannot resume owned process")
                        return
                    finally:
                        self.api.CloseHandle(thread)
                present = self.api.Thread32Next(snapshot, ctypes.byref(entry))
            raise RuntimeError("Owned suspended process has no primary thread")
        finally:
            self.api.CloseHandle(snapshot)

    def close(self):
        if self.handle:
            self.api.CloseHandle(self.handle)
            self.handle = None


class Harness:
    def __init__(self, root, binary, codex, model, codex_home, timeout):
        self.root, self.binary, self.codex = root, binary, codex
        self.model, self.codex_home, self.timeout = model, codex_home, timeout
        self.process = self.tree = None
        self.logs = []
        self.starts = 0
        self.http = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        self.manifest_path = root / "smoke-manifest.json"
        self.manifest = json.loads(self.manifest_path.read_text(encoding="utf-8"))

    def start(self):
        self.starts += 1
        stdout = self.root / f"stdout-{self.starts}-{uuid.uuid4().hex[:8]}.log"
        stderr = stdout.with_name(stdout.name.replace("stdout", "stderr"))
        self.logs = [stdout.open("w", encoding="utf-8"), stderr.open("w", encoding="utf-8")]
        environment = os.environ.copy()
        environment.update(FORGE_AGENTD_DATA_DIR=str(self.root / "data"),
                           FORGE_GEN_DATA_DIR=str(self.root / "gen"),
                           FORGE_AGENTD_ADDR="127.0.0.1:0", FORGE_CODEX_BIN=str(self.codex))
        if self.codex_home:
            environment["CODEX_HOME"] = str(self.codex_home)
        self.process = subprocess.Popen([str(self.binary)], cwd=self.root / "workspace", env=environment,
                                        stdin=subprocess.DEVNULL, stdout=self.logs[0], stderr=self.logs[1],
                                        creationflags=subprocess.CREATE_NO_WINDOW | 0x4)  # CREATE_SUSPENDED
        self.tree = ProcessTree(self.process)
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            require(self.process.poll() is None, "Isolated daemon exited during startup; see its local stderr log")
            text = stdout.read_text(encoding="utf-8", errors="replace")
            if "listening at http://" in text:
                self.base = text.split("listening at ", 1)[1].splitlines()[0].strip()
                require(self.base.startswith("http://127.0.0.1:"), "Daemon did not bind loopback")
                return
            time.sleep(0.1)
        raise RuntimeError("Isolated daemon startup timed out")

    def stop(self):
        if self.tree:
            self.tree.close()
            self.tree = None
        if self.process:
            self.process.wait(timeout=10)
            self.process = None
        for log in self.logs:
            log.close()
        self.logs = []

    def api(self, path, body=None):
        request = urllib.request.Request(self.base + path, data=None if body is None else json.dumps(body).encode(),
                                         headers={"Content-Type": "application/json"})
        try:
            with self.http.open(request, timeout=self.timeout) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            # Error bodies can contain upstream diagnostics: retain only HTTP status.
            raise RuntimeError(f"Forge HTTP {error.code} for {path.split('/')[3]}") from None

    def session(self, title):
        return self.api("/api/forge/sessions", {
            "title": title, "workspaceId": self.manifest["workspaceId"], "agentEngine": "codex",
            "agentKind": "coding", "selectedModelId": f"codex:{self.model}",
            "reasoningEffort": "low", "webSearchEnabled": False,
        })["session"]["id"]

    def ask(self, sid, mode, prompt):
        result = self.api(f"/api/forge/sessions/{sid}/ask:execute", {
            "mode": mode, "includeLibrary": False, "userInput": prompt,
        })
        require(result.get("run", {}).get("status") == "completed", f"{mode} run did not complete; inspect isolated local events")
        return result

    def agents(self, sid):
        return self.api(f"/api/forge/sessions/{sid}/agents")["agents"]

    def messages(self, sid, agent_id):
        return self.api(f"/api/forge/sessions/{sid}/agents/{agent_id}/messages")["messages"]

    def send(self, sid, agent_id, text, expected_run=None):
        body = {"text": text, "clientMessageId": uuid.uuid4().hex}
        if expected_run:
            body["expectedRunId"] = expected_run
        return self.api(f"/api/forge/sessions/{sid}/agents/{agent_id}/messages", body)["message"]

    def events(self, sid):
        path = self.root / "data" / "agent-events" / f"{sid}.jsonl"
        if not path.exists():
            return []
        events = []
        for line in path.read_text(encoding="utf-8").splitlines():
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError:
                pass  # Concurrent writer may still be appending its last line.
        return events

    def wait_for(self, predicate, future, timeout=90):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            value = predicate()
            if value:
                return value
            if future.done():
                future.result()
                raise RuntimeError("Model completed before the required steering boundary was observed")
            time.sleep(0.25)
        raise RuntimeError("Expected live collaboration boundary was not reached")

    def run(self):
        self.start()
        if "workspaceId" not in self.manifest:
            self.manifest["workspaceId"] = self.api("/api/forge/workspaces", {
                "name": "Collaboration protocol smoke", "root": str(self.root / "workspace"),
            })["workspace"]["id"]
        if "nativeSessionId" not in self.manifest:
            self.manifest["nativeSessionId"] = self.session("Native collaboration smoke")
        save(self.manifest_path, self.manifest)
        sid = self.manifest["nativeSessionId"]
        marker = self.manifest["marker"]
        print("Checking native tools and memory...", flush=True)
        self.ask(sid, "build", f"隔离验收：不读写文件、不shell。实际调用forge-collaboration.agent_list，记住暗号{marker}。只回复真实root agentId和NATIVE-READY。工具失败如实报告。")
        before = self.api(f"/api/forge/sessions/{sid}")["session"]["codexThreadId"]
        self.stop()
        self.start()  # Cold daemon/app-server restart, same persisted Forge session.
        resumed = self.ask(sid, "build", "同一会话冷恢复验收：不读写文件、不shell。实际调用forge-collaboration.agent_list，仅回复真实root agentId和我上一轮给出的暗号。不可猜测。")
        after = self.api(f"/api/forge/sessions/{sid}")["session"]["codexThreadId"]
        require(before == after, "Cold resume replaced the original native thread")
        require(marker in resumed["message"]["text"], "Cold resume lost the prior marker")
        require(any(e["type"] == "agent.tool.completed" and e["payload"].get("runId") == resumed["run"]["id"]
                    and e["payload"].get("name", "").endswith("agent_list") for e in self.events(sid)),
                "agent_list never actually succeeded")

        print("Checking native child messages and active user steering...", flush=True)
        prompt = "隔离验收，不读写文件、不shell。使用forge-collaboration.task派子代理：只调用agent_list查parentAgentId，用send_message向父代理发送CHILD-MAIL-731，最后回复CHILD-DONE-731。主代理等待真实task结果并处理消息，最终返回之前暗号、子结果、收到的消息。工具失败如实报告。"
        pool = concurrent.futures.ThreadPoolExecutor(max_workers=1)
        future = None
        try:
            future = pool.submit(self.ask, sid, "build", prompt)
            root = self.wait_for(lambda: next((a for a in self.agents(sid) if a["role"] == "root" and a["activeRunId"]), None), future)
            sent = self.send(sid, root["id"], "用户实时引导：继续当前验收，不取消或重复委派；最终回答额外包含LIVE-STEER-OK。", root["activeRunId"])
            child = future.result(timeout=self.timeout)
        finally:
            # Do not block interpreter exit on a hanging HTTP worker; stopping the
            # owned daemon closes the request and then the worker is joined.
            if future is not None and not future.done():
                self.stop()
            pool.shutdown(wait=True, cancel_futures=True)
        for token in (marker, "CHILD-MAIL-731", "CHILD-DONE-731", "LIVE-STEER-OK"):
            require(token in child["message"]["text"], f"Native result is missing {token}")
        messages = self.messages(sid, root["id"])
        require(any(x["id"] == sent["id"] and x["status"] == "injected" for x in messages), "User steering was not acknowledged")
        require(any(x["source"] == "agent" and x["text"] == "CHILD-MAIL-731" and x["status"] == "injected" for x in messages), "Child message was not acknowledged")

        print("Checking persistent team, dependency order, peer mail and idle wake...", flush=True)
        team_sid = self.session("Managed team collaboration smoke")
        self.manifest["teamSessionId"] = team_sid
        save(self.manifest_path, self.manifest)
        prompt = "隔离协作验收，所有代理禁止读写文件及shell，只用协作工具。使用当前Team，先创建通用默认的具名持久成员Alpha和Beta，再plan_write创建指定ownerAgentId的三个任务：A由Alpha记住IRIS-482，agent_list并向父代理send_message ALPHA-READY，返回A-OK；B由Beta执行且deps=[A]，agent_list找到Alpha，send_message给它PEER-CORRECTION，返回B-OK；C由原Alpha执行且deps=[B]，仅凭成员保留上下文回忆暗号和同伴消息，返回C-OK及两者，C描述不能重复暗号。严格A→B→C，不同步task，不重建成员。全部完成后team_control complete，最终列真实结果和上下文复用情况，失败如实报告。"
        pool = concurrent.futures.ThreadPoolExecutor(max_workers=1)
        future = None
        try:
            future = pool.submit(self.ask, team_sid, "team", prompt)
            alpha = self.wait_for(lambda: next((a for a in self.agents(team_sid) if a["name"] == "Alpha"), None), future)
            member_message = self.send(team_sid, alpha["id"], "用户引导：不改变依赖计划；后续C最终报告额外包含USER-MEMBER-STEER。")
            team_result = future.result(timeout=self.timeout)
        finally:
            if future is not None and not future.done():
                self.stop()
            pool.shutdown(wait=True, cancel_futures=True)
        team = self.api(f"/api/forge/sessions/{team_sid}/team")["team"]
        tasks = {task["id"]: task for task in team["tasks"]}
        require(team["status"] == "completed" and set(tasks) == {"A", "B", "C"}, "Expected completed three-task team")
        require(all(t["status"] == "completed" and t["attempts"] == 1 for t in tasks.values()), "Task was replayed or failed")
        require(tasks["B"]["deps"] == ["A"] and tasks["C"]["deps"] == ["B"], "Dependency graph was not preserved")
        require(tasks["A"]["updatedAt"] <= tasks["B"]["updatedAt"] <= tasks["C"]["updatedAt"], "Dependency completion order was violated")
        require(tasks["A"]["ownerAgentId"] == tasks["C"]["ownerAgentId"] == alpha["id"], "Alpha identity changed between tasks")
        for token in ("IRIS-482", "PEER-CORRECTION", "USER-MEMBER-STEER"):
            require(token in tasks["C"]["result"], f"Persistent member lost {token}")
        require("IRIS-482" not in tasks["C"]["prompt"], "C prompt leaked the memory-test marker")
        mail = self.messages(team_sid, alpha["id"])
        require(any(x["id"] == member_message["id"] and x["status"] == "injected" for x in mail), "Member user message was not acknowledged")
        require(any(x["toAgentId"] == alpha["id"] and x["text"] == "PEER-CORRECTION" and x["status"] == "injected" for x in mail), "Peer message was not acknowledged")
        jobs = [json.loads(path.read_text(encoding="utf-8")) for path in (self.root / "data" / "agent-sessions" / "collaboration-jobs").glob("*.json")]
        alpha_job = next(job for job in jobs if job["jobId"] == alpha["id"])
        require(alpha_job["generation"] >= 2 and alpha_job["threadId"], "Alpha did not reuse a persistent thread")
        evidence = {}
        for label, current_sid in (("native", sid), ("team", team_sid)):
            events = self.events(current_sid)
            counts = collections.Counter(e["payload"].get("name") for e in events if e["type"] == "agent.tool.invoked")
            unexpected = [name for name in counts if not isinstance(name, str)
                          or name.removeprefix("mcp__forge-collaboration__") not in ALLOWED_TOOLS]
            require(not unexpected, f"Unexpected non-collaboration tools: {unexpected}")
            evidence[label] = dict(counts)
        return {"native": {"sameThreadId": before, "coldResumeResult": resumed, "childAndSteerResult": child},
                "team": {"id": team["id"], "status": team["status"], "tasks": list(tasks.values()), "result": team_result,
                         "alphaThreadId": alpha_job["threadId"], "alphaGenerations": alpha_job["generation"]},
                "toolCounts": evidence}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--agentd", type=Path, required=True, help="Existing built forge-agentd.exe; copied before use")
    parser.add_argument("--codex", type=Path, required=True, help="Existing Codex executable; never downloaded")
    parser.add_argument("--model", default="gpt-6.1-sol")
    parser.add_argument("--codex-home", type=Path, help="Optional existing authenticated CODEX_HOME; never created/copied")
    parser.add_argument("--resume-root", type=Path, help="Previous isolated directory created by this script")
    parser.add_argument("--evidence", type=Path, help="Optional NEW output JSON file; existing evidence is never overwritten")
    parser.add_argument("--timeout", type=int, default=360, help="Per model-turn HTTP timeout in seconds")
    args = parser.parse_args()
    require(os.name == "nt", "This acceptance harness currently requires Windows process-job isolation")
    binary, codex = args.agentd.resolve(strict=True), args.codex.resolve(strict=True)
    require(binary.is_file() and codex.is_file(), "Both executable paths must name existing files")
    if args.codex_home:
        require(args.codex_home.is_dir(), "--codex-home must already exist")
    require(args.timeout >= 30, "--timeout must be at least 30 seconds")
    if args.evidence:
        require(not args.evidence.exists(), "Refusing to overwrite existing evidence")
        require(args.evidence.parent.is_dir(), "Evidence parent directory must already exist")
    parent = Path(os.environ["LOCALAPPDATA"]) / "Temp"
    require(parent.resolve().drive.upper() == "C:", "Default scratch location must be on C:")
    if args.resume_root:
        root = args.resume_root.resolve(strict=True)
        require(root.parent == parent.resolve() and root.name.startswith("rurixforge-collaboration-smoke-"), "Resume directory is not owned smoke scratch")
        manifest = json.loads((root / "smoke-manifest.json").read_text(encoding="utf-8"))
        require(manifest.get("schema") == SCHEMA, "Resume marker is invalid")
        require(manifest["model"] == args.model, "Resume must use the original smoke model")
        require(all((root / name).is_dir() and (root / name).resolve().parent == root for name in ("data", "gen", "workspace")), "Scratch paths escaped the owned directory")
        require(all(not (path.lstat().st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT)
                    for path in (args.resume_root.absolute(), root / "smoke-manifest.json", root / "data", root / "gen", root / "workspace")),
                "Resume scratch must not contain redirected paths")
    else:
        root = Path(tempfile.mkdtemp(prefix="rurixforge-collaboration-smoke-", dir=parent))
        for name in ("data", "gen", "workspace"):
            (root / name).mkdir()
        save(root / "smoke-manifest.json", {"schema": SCHEMA, "model": args.model, "marker": f"ORCHID-{uuid.uuid4().hex[:8]}"})
    copied = root / f"forge-agentd-{uuid.uuid4().hex[:8]}.exe"
    shutil.copy2(binary, copied)
    save(root / "data" / "codex-config.json", {"authSource": "chatgpt", "autoRegisterMcp": False,
         "defaultModel": args.model, "defaultEngine": "local", "codexHome": str(args.codex_home.resolve()) if args.codex_home else ""})
    version = subprocess.run([str(codex), "--version"], capture_output=True, text=True, check=True, timeout=15).stdout.strip()
    report = {"schema": SCHEMA, "createdAt": dt.datetime.now(dt.timezone.utc).isoformat(), "cliVersion": version,
              "model": args.model, "daemonSha256": hashlib.sha256(copied.read_bytes()).hexdigest(), "status": "failed"}
    harness = Harness(root, copied, codex, args.model, args.codex_home, args.timeout)
    print(f"Isolated verification directory: {root}", flush=True)
    try:
        report.update(harness.run())
        report["status"] = "passed"
    except BaseException as error:
        report["error"] = str(error)
        raise
    finally:
        try:
            harness.stop()
            report["daemonStopped"] = True
        except BaseException:
            report["daemonStopped"] = False
            report["status"] = "failed"
            report["cleanupError"] = "Owned daemon did not confirm exit before the cleanup timeout"
            raise
        finally:
            save(root / f"evidence-{uuid.uuid4().hex[:8]}.json", report, exclusive=True)
            if args.evidence:
                save(args.evidence, report, exclusive=True)
            print(f"Result: {report['status']}; daemonStopped={report.get('daemonStopped', False)}. Evidence retained in {root}", flush=True)


if __name__ == "__main__":
    main()
