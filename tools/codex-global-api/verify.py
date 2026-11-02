"""Verify the configured Codex API with bounded, real inference requests.

Reads ~/.config/rurixforge/codex-api.json; never prints credentials or raw
responses. The default run checks all seven models and a function-tool loop.
Each inference runs once, without automatic retries. --quick checks only the
default model and skips the tool loop, while retaining authentication and SSE.
"""
from __future__ import annotations

import argparse
import datetime as dt
import http.client
import json
import os
import socket
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[2]
TIMEOUT = 120.0
MAX_RESPONSE = 8 * 1024 * 1024
TEXT_MARKER = "RF_OK"
TOOL_MARKER = "RF_TOOL_OK"


@dataclass
class Reply:
    status: int
    payload: Any
    elapsed_ms: int
    error: str = ""
    event_stream: bool = False


class Client:
    def __init__(self, config: dict[str, Any]):
        self.url = urlsplit(config["baseUrl"].rstrip("/"))
        if (
            self.url.scheme not in {"http", "https"}
            or not self.url.hostname
            or self.url.username
            or self.url.password
            or self.url.query
            or self.url.fragment
        ):
            raise ValueError("invalid_base_url")
        self.key = config["apiKey"]

    def call(self, method: str, path: str, body: dict | None = None,
             auth: str = "valid", stream: bool = False) -> Reply:
        started = time.monotonic()
        connection_type = (
            http.client.HTTPSConnection if self.url.scheme == "https"
            else http.client.HTTPConnection
        )
        connection = connection_type(self.url.hostname, self.url.port, timeout=TIMEOUT)
        status = 0
        headers = {"Content-Type": "application/json",
                   "User-Agent": "rurixforge-codex-api-verifier/1.0"}
        if auth != "none":
            token = self.key if auth == "valid" else "rf-intentionally-invalid-smoke-key"
            headers["Authorization"] = "Bearer " + token
        headers["Accept"] = "text/event-stream" if stream else "application/json"
        try:
            data = None if body is None else json.dumps(body, ensure_ascii=False).encode("utf-8")
            connection.request(method, self.url.path + path, body=data, headers=headers)
            if connection.sock is not None:
                connection.sock.settimeout(max(0.001, TIMEOUT - (time.monotonic() - started)))
            response = connection.getresponse()
            status = response.status
            event_stream = "text/event-stream" in response.getheader("Content-Type", "").lower()
            chunks = []
            size = 0
            while True:
                remaining = TIMEOUT - (time.monotonic() - started)
                if remaining <= 0:
                    raise TimeoutError
                # getresponse() can close the connection socket when the peer
                # declares Connection: close, leaving buffered response bytes.
                # Never reuse that closed socket; the response retains its
                # original timeout in this case.
                if connection.sock is not None:
                    connection.sock.settimeout(remaining)
                chunk = response.read1(64 * 1024)
                if not chunk:
                    break
                size += len(chunk)
                if size > MAX_RESPONSE:
                    return Reply(status, None, elapsed(started), "response_too_large", event_stream)
                chunks.append(chunk)
            raw = b"".join(chunks)
            if stream and status == 200:
                payload = parse_sse(raw)
            else:
                payload = json.loads(raw) if raw else {}
            return Reply(status, payload, elapsed(started), event_stream=event_stream)
        except (TimeoutError, socket.timeout):
            return Reply(status, None, elapsed(started), "request_timeout")
        except (UnicodeError, ValueError, json.JSONDecodeError):
            return Reply(status, None, elapsed(started), "invalid_response")
        except (OSError, http.client.HTTPException):
            return Reply(status, None, elapsed(started), "transport_failure")
        finally:
            connection.close()


def elapsed(started: float) -> int:
    return round((time.monotonic() - started) * 1000)


def parse_sse(raw: bytes) -> list[Any]:
    events = []
    data = []
    # Keep multi-line data frames intact and accept either LF or CRLF.
    for line in raw.decode("utf-8").splitlines() + [""]:
        if line == "":
            if data:
                joined = "\n".join(data)
                events.append("[DONE]" if joined == "[DONE]" else json.loads(joined))
                data = []
        elif line.startswith("data:"):
            value = line[5:]
            data.append(value[1:] if value.startswith(" ") else value)
    return events


def response_text(payload: Any) -> str:
    if not isinstance(payload, dict):
        return ""
    return "".join(
        part.get("text", "")
        for item in payload.get("output", []) if isinstance(item, dict)
        for part in item.get("content", []) if isinstance(part, dict)
        if part.get("type") == "output_text" and isinstance(part.get("text"), str)
    ).strip()


def response_body(model: str, **extra: Any) -> dict[str, Any]:
    return {"model": model, "input": "Reply only RF_OK", "stream": False,
            "reasoning": {"effort": "low"}, **extra}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path,
                        default=Path(os.environ.get("USERPROFILE", str(Path.home())))
                        / ".config/rurixforge/codex-api.json")
    parser.add_argument("--report-dir", type=Path)
    parser.add_argument("--quick", action="store_true")
    args = parser.parse_args()
    now = dt.datetime.now(dt.timezone.utc)
    report_dir = args.report_dir or ROOT / "evidence" / (
        "codex-global-api-" + now.strftime("%Y%m%dT%H%M%S%fZ"))
    report: dict[str, Any] = {
        "schema": "rurixforge.codex-global-api.verification.v1",
        "createdAtUtc": now.isoformat(), "quick": args.quick,
        "requestTimeoutSeconds": TIMEOUT, "automaticRetries": 0,
        "checks": [], "skipped": [],
    }
    try:
        config = json.loads(args.config.read_text(encoding="utf-8-sig"))
        models = config["models"]
        model = config["model"]
        if (not isinstance(models, list) or len(models) != 7
                or not all(isinstance(m, str) and m for m in models)
                or len(set(models)) != 7 or model not in models
                or not isinstance(config["apiKey"], str) or not config["apiKey"]):
            raise ValueError("invalid_client_config")
        client = Client(config)
        report["defaultModel"] = model
        report["configuredModels"] = models
    except (OSError, ValueError, KeyError, TypeError):
        report["checks"].append({"check": "client_config", "passed": False,
                                  "error": "missing_or_invalid_client_config"})
        return save_report(report, report_dir)

    def record(name: str, reply: Reply, conditions: dict[str, bool],
               selected: str | None = None) -> bool:
        passed = not reply.error and all(conditions.values())
        entry = {"check": name, "passed": passed, "httpStatus": reply.status,
                 "latencyMs": reply.elapsed_ms, "conditions": conditions}
        if selected:
            entry["model"] = selected
        if reply.error:
            entry["error"] = reply.error
        report["checks"].append(entry)
        print(("PASS " if passed else "FAIL ") + name
              + (" " + selected if selected else ""), flush=True)
        return passed

    for auth in ("none", "invalid"):
        result = client.call("GET", "/models", auth=auth)
        record("reject_" + auth + "_key", result, {"unauthorized": result.status == 401})

    result = client.call("GET", "/models")
    payload = result.payload if isinstance(result.payload, dict) else {}
    listed = [m.get("id") for m in payload.get("data", []) if isinstance(m, dict)]
    record("model_catalog", result, {
        "http_ok": result.status == 200, "openai_list": payload.get("object") == "list",
        "seven_expected_models": len(listed) == 7 and set(listed) == set(models),
    })
    chosen = [model] if args.quick else models
    if args.quick:
        report["skipped"] = ["other_models", "function_tool_roundtrip"]
    for selected in chosen:
        result = client.call("POST", "/responses", response_body(selected))
        payload = result.payload if isinstance(result.payload, dict) else {}
        record("responses_nonstream", result, {
            "http_ok": result.status == 200, "completed": payload.get("status") == "completed",
            "marker_matches": response_text(payload) == TEXT_MARKER,
        }, selected)

    chat = {"model": model, "messages": [{"role": "user", "content": "Reply only RF_OK"}],
            "stream": False, "reasoning_effort": "low"}
    result = client.call("POST", "/chat/completions", chat)
    payload = result.payload if isinstance(result.payload, dict) else {}
    choices = payload.get("choices") or []
    choice = choices[0] if choices and isinstance(choices[0], dict) else {}
    record("chat_nonstream", result, {
        "http_ok": result.status == 200, "chat_object": payload.get("object") == "chat.completion",
        "stopped": choice.get("finish_reason") == "stop",
        "marker_matches": (choice.get("message") or {}).get("content", "").strip() == TEXT_MARKER,
    }, model)

    result = client.call("POST", "/responses", response_body(model, stream=True), stream=True)
    events = result.payload if isinstance(result.payload, list) else []
    objects = [e for e in events if isinstance(e, dict)]
    terminal = [e for e in objects if e.get("type") == "response.completed"]
    deltas = [e.get("delta", "") for e in objects if e.get("type") == "response.output_text.delta"]
    record("responses_stream", result, {
        "http_ok": result.status == 200, "sse_content_type": result.event_stream,
        "has_text_delta": bool(deltas), "marker_matches": "".join(deltas).strip() == TEXT_MARKER,
        "completed": bool(terminal) and terminal[-1].get("response", {}).get("status") == "completed",
        "no_error_event": not any(e.get("type") in {"error", "response.failed", "response.incomplete"}
                                  for e in objects),
    }, model)

    result = client.call("POST", "/chat/completions", {**chat, "stream": True}, stream=True)
    events = result.payload if isinstance(result.payload, list) else []
    chunks = [e for e in events if isinstance(e, dict)]
    stream_choices = [c for e in chunks for c in e.get("choices", []) if isinstance(c, dict)]
    text = "".join((c.get("delta") or {}).get("content") or "" for c in stream_choices)
    record("chat_stream", result, {
        "http_ok": result.status == 200, "sse_content_type": result.event_stream,
        "done": bool(events) and events[-1] == "[DONE]",
        "stopped": any(c.get("finish_reason") == "stop" for c in stream_choices),
        "marker_matches": text.strip() == TEXT_MARKER,
        "no_error_event": not any("error" in e for e in chunks),
    }, model)

    if not args.quick:
        verify_tools(client, model, record)
    return save_report(report, report_dir)


def verify_tools(client: Client, model: str, record: Any) -> None:
    prompt = [{"role": "user", "content": "Call rf_echo_marker with marker RF_TOOL_OK. "
               "After the tool returns, reply only with the marker returned by the tool."}]
    tool = {"type": "function", "name": "rf_echo_marker", "strict": True,
            "description": "Return the supplied verification marker.",
            "parameters": {"type": "object", "properties": {"marker": {"type": "string"}},
                           "required": ["marker"], "additionalProperties": False}}
    body = response_body(model, input=prompt, tools=[tool], include=["reasoning.encrypted_content"],
                         tool_choice={"type": "function", "name": tool["name"]})
    result = client.call("POST", "/responses", body)
    payload = result.payload if isinstance(result.payload, dict) else {}
    output = payload.get("output", [])
    calls = [item for item in output if isinstance(item, dict) and item.get("type") == "function_call"]
    valid_calls = bool(calls)
    for call in calls:
        try:
            arguments = json.loads(call.get("arguments", ""))
            valid_calls = valid_calls and call.get("name") == tool["name"] \
                and bool(call.get("call_id")) and arguments == {"marker": TOOL_MARKER}
        except (ValueError, TypeError):
            valid_calls = False
    if not record("function_tool_requested", result, {
        "http_ok": result.status == 200, "completed": payload.get("status") == "completed",
        "valid_function_call": valid_calls,
    }, model):
        return
    outputs = [{"type": "function_call_output", "call_id": call["call_id"],
                "output": json.dumps({"marker": TOOL_MARKER})} for call in calls]
    followup = response_body(model, input=prompt + output + outputs, include=["reasoning.encrypted_content"],
                             tools=[tool], tool_choice="none")
    result = client.call("POST", "/responses", followup)
    payload = result.payload if isinstance(result.payload, dict) else {}
    record("function_tool_roundtrip", result, {
        "http_ok": result.status == 200, "completed": payload.get("status") == "completed",
        "marker_matches": response_text(payload) == TOOL_MARKER,
    }, model)


def save_report(report: dict[str, Any], report_dir: Path) -> int:
    checks = report["checks"]
    report["passed"] = bool(checks) and all(check["passed"] for check in checks)
    report["summary"] = {"passed": sum(check["passed"] for check in checks),
                         "failed": sum(not check["passed"] for check in checks),
                         "total": len(checks)}
    report_dir.mkdir(parents=True, exist_ok=True)
    path = report_dir / "verification.json"
    # The report schema deliberately excludes response bodies, prompts, headers,
    # account identifiers, credentials, and upstream error messages.
    path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print("Report: " + str(path), flush=True)
    print(json.dumps(report["summary"]), flush=True)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
