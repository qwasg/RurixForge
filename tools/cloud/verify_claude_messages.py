"""Exercise the deployed Claude APIs using an isolated test-account device key.

Only status, lengths, timing and usage are written to evidence. Tokens and signed
assistant blocks remain in memory; logout revokes the temporary device key.
"""
import json
import re
import time
import urllib.error
import urllib.request
import uuid
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone

from configure_claude_messages import ROOT, ORIGIN, MODEL_IDS, api

PROMPT = "Find the smallest positive integer x satisfying x mod 7 = 3, x mod 11 = 4, x mod 13 = 5. Verify it and give a brief answer."


def request(path, body, key, stream=False):
    headers = {"Content-Type": "application/json", "anthropic-version": "2023-06-01"}
    if path == "/v1/messages":
        headers["x-api-key"] = key
    else:
        headers["Authorization"] = "Bearer " + key
    start = time.monotonic()
    req = urllib.request.Request(ORIGIN + path, data=json.dumps(body).encode(), headers=headers)
    with urllib.request.urlopen(req, timeout=180) as response:
        first = None
        if not stream:
            payload = json.load(response)
            blocks = payload.get("content", []) if path == "/v1/messages" else payload["choices"][0]["message"].get("anthropic_content", [])
            message = payload if path == "/v1/messages" else payload["choices"][0]["message"]
            text = "".join(b.get("text", "") for b in blocks) if path == "/v1/messages" else message.get("content", "") or ""
            thinking = "".join(b.get("thinking", "") for b in blocks) if path == "/v1/messages" else message.get("reasoning_content", "")
            result = {"status": response.status, "seconds": round(time.monotonic()-start, 2),
                      "blocks": [b.get("type") for b in blocks], "textChars": len(text), "thinkingChars": len(thinking),
                      "signedThinking": any(b.get("signature") for b in blocks), "usage": payload.get("usage")}
            return result, payload
        events = []; thinking = ""; text = ""; signature_count = 0; usage = None; done = False
        for line in response:
            line = line.decode("utf-8").strip()
            if not line.startswith("data:"):
                continue
            if first is None:
                first = round(time.monotonic()-start, 2)
            data = line[5:].strip()
            if data == "[DONE]":
                done = True; break
            event = json.loads(data)
            if "error" in event:
                raise RuntimeError("Upstream stream error: " + str(event["error"]).replace(key, "[redacted]"))
            if path == "/v1/messages":
                events.append(event.get("type"))
                delta = event.get("delta", {})
                thinking += delta.get("thinking", "")
                text += delta.get("text", "")
                signature_count += bool(delta.get("signature"))
                if event.get("usage"):
                    usage = event["usage"]
                if event.get("type") == "message_stop":
                    done = True
            else:
                for choice in event.get("choices", []):
                    delta = choice.get("delta", {})
                    thinking += delta.get("reasoning_content", "")
                    text += delta.get("content", "") or ""
                    signature_count += sum(bool(b.get("signature")) for b in delta.get("anthropic_content", []))
                if event.get("usage"):
                    usage = event["usage"]
        return {"status": response.status, "seconds": round(time.monotonic()-start, 2), "firstEventSeconds": first,
                "textChars": len(text), "thinkingChars": len(thinking), "signatureCount": signature_count,
                "done": done, "events": sorted(set(events)), "usage": usage}, None


def native_body(model):
    body = {"model": model, "max_tokens": 4096, "messages": [{"role": "user", "content": PROMPT}]}
    if model == "claude-haiku-4-5":
        body["thinking"] = {"type": "enabled", "budget_tokens": 1024, "display": "summarized"}
    else:
        body["thinking"] = {"type": "adaptive", "display": "summarized"}
        body["output_config"] = {"effort": "low"}
    return body


def check(model, protocol, key, stream=False):
    path = "/v1/messages" if protocol == "messages" else "/v1/chat/completions"
    body = native_body(model) if protocol == "messages" else {
        "model": model, "messages": [{"role":"user", "content": PROMPT}],
        "max_tokens": 4096, "thinking_enabled": True, "reasoning_effort": "low",
    }
    body["stream"] = stream
    if protocol == "chat" and stream:
        body["stream_options"] = {"include_usage": True}
    result, _ = request(path, body, key, stream)
    result.update({"model": model, "protocol": protocol, "stream": stream})
    result["ok"] = result["status"] == 200 and result["textChars"] > 0 and result["thinkingChars"] > 0
    if stream:
        result["ok"] = result["ok"] and result["done"] and result["signatureCount"] > 0
    else:
        result["ok"] = result["ok"] and result["signedThinking"]
    return result


def tool_round_trip(protocol, key):
    model = "claude-sonnet-5-5"
    path = "/v1/messages" if protocol == "messages" else "/v1/chat/completions"
    tool = {"name": "lookup_value", "description": "Read a value by key. Values are not known without this tool.",
            "input_schema": {"type":"object", "properties":{"key":{"type":"string"}}, "required":["key"]}}
    body = native_body(model) if protocol == "messages" else {
        "model":model, "max_tokens":4096, "thinking_enabled":True, "reasoning_effort":"low",
    }
    body["messages"] = [{"role":"user", "content":"Call lookup_value with key alpha, then report only the returned value. Use the tool; do not guess."}]
    body["tools"] = [tool] if protocol == "messages" else [{"type":"function", "function":{
        "name":tool["name"], "description":tool["description"], "parameters":tool["input_schema"]}}]
    body["tool_choice"] = {"type":"auto"} if protocol == "messages" else "auto"
    first, payload = request(path, body, key)
    assistant = payload if protocol == "messages" else payload["choices"][0]["message"]
    blocks = assistant.get("content", []) if protocol == "messages" else assistant.get("anthropic_content", [])
    tool_uses = [b for b in blocks if b.get("type") == "tool_use"]
    if not tool_uses:
        raise RuntimeError("Tool round trip returned no tool_use")
    signed = any(b.get("signature") for b in blocks)
    if protocol == "messages":
        body["messages"].append({"role":"assistant", "content":blocks})
        body["messages"].append({"role":"user", "content":[{"type":"tool_result", "tool_use_id":t["id"], "content":"23"} for t in tool_uses]})
    else:
        body["messages"].append(assistant)
        body["messages"].extend({"role":"tool", "tool_call_id":t["id"], "content":"23"} for t in tool_uses)
    second, payload2 = request(path, body, key)
    text = ("".join(b.get("text", "") for b in payload2.get("content", [])) if protocol == "messages" else
            payload2["choices"][0]["message"].get("content", "") or "")
    return {"model":model, "protocol":protocol, "toolRoundTrip":True, "signedFirstTurn":signed,
            "first":first, "second":second, "ok":signed and "23" in text}


def main():
    secret = json.loads((ROOT / "data/cloud-server/credentials.json").read_text(encoding="utf-8-sig"))
    login = api("POST", "/api/v1/auth/login", {
        "email":secret["testEmail"], "password":secret["testPassword"],
        "device":{"id":"claude-verify-"+str(uuid.uuid4()), "name":"Claude API verification", "platform":"windows", "appVersion":"0.1.0"},
        "issueDeviceKey":True,
    })
    key = login["deviceKey"]["key"]
    results = []
    try:
        tasks = [(model, protocol, False) for model in MODEL_IDS for protocol in ["messages", "chat"]]
        tasks += [("claude-sonnet-5-5", protocol, True) for protocol in ["messages", "chat"]]
        with ThreadPoolExecutor(max_workers=2) as executor:
            futures = {executor.submit(check, model, protocol, key, stream): (model,protocol,stream)
                       for model,protocol,stream in tasks}
            for future in as_completed(futures):
                try:
                    result = future.result()
                except Exception as error:
                    message = str(error).replace(key,"[redacted]")
                    if isinstance(error, urllib.error.HTTPError):
                        message += " " + error.read().decode(errors="replace").replace(key,"[redacted]")
                    model,protocol,stream = futures[future]
                    result = {"model":model, "protocol":protocol, "stream":stream, "ok":False,
                              "error":re.sub(r"sk-[A-Za-z0-9_-]+","[redacted]",message)[:1200]}
                results.append(result)
                print(json.dumps(result,ensure_ascii=False),flush=True)
        for protocol in ["messages", "chat"]:
            result = tool_round_trip(protocol,key)
            results.append(result)
            print(json.dumps(result,ensure_ascii=False),flush=True)
    finally:
        api("POST", "/api/v1/auth/logout", {}, login["accessToken"])
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        (ROOT / "evidence/cloud-server-20261007" / f"claude-messages-verified-{stamp}.json").write_text(
            json.dumps({"createdAtUTC":stamp, "temporaryDeviceKeyRevoked":True, "results":results},ensure_ascii=False,indent=2),encoding="utf-8")
    if len(results) != 12 or not all(r["ok"] for r in results):
        raise SystemExit("Claude API verification incomplete or failed")


if __name__ == "__main__":
    main()
