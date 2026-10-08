"""Server-side smoke test for local-model-gateway.

1. GET  /health
2. GET  /v1/models            (model list pull through the gateway)
3. per model: a minimal call through the gateway
   - embedding models  -> POST /v1/embeddings
   - everything else   -> POST /v1/chat/completions (max_tokens=16)
Writes a JSON report to smoke_report.json (no secrets in it).

Usage: python smoke_test.py [--only-list] [--workers N]
"""
from __future__ import annotations

import argparse
import json
import os
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
for line in (HERE / ".env").read_text(encoding="utf-8").splitlines():
    if "=" in line and not line.lstrip().startswith("#"):
        k, v = line.split("=", 1)
        os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))

BASE = f"http://{os.environ.get('HOST', '127.0.0.1')}:{os.environ.get('PORT', '8787')}"
TOKEN = os.environ["LOCAL_API_KEY"]


def call(method: str, path: str, body: dict | None = None, timeout: float = 120):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(BASE + path, data=data, method=method, headers={
        "Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json"})
    t0 = time.time()
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read()
            status = r.status
    except urllib.error.HTTPError as e:
        raw, status = e.read(), e.code
    except Exception as e:
        return 0, {"error": f"{e.__class__.__name__}: {e}"}, time.time() - t0
    try:
        payload = json.loads(raw)
    except ValueError:
        payload = {"raw": raw[:300].decode("utf-8", "replace")}
    return status, payload, time.time() - t0


def test_model(mid: str) -> dict:
    low = mid.lower()
    if "embed" in low:
        st, p, dt = call("POST", "/v1/embeddings", {"model": mid, "input": "ping"})
        ok = st == 200 and bool(p.get("data"))
        detail = f"dim={len(p['data'][0]['embedding'])}" if ok else p
        return {"model": mid, "kind": "embedding", "status": st, "ok": ok,
                "latency_s": round(dt, 2), "detail": detail if ok else _err(detail)}
    if "image" in low:
        st, p, dt = call("POST", "/v1/images/generations", {
            "model": mid, "prompt": "a small red circle on white background",
            "n": 1, "size": "1024x1024", "quality": "low"}, timeout=300)
        d = (p.get("data") or [{}])[0] if st == 200 else {}
        ok = st == 200 and bool(d.get("b64_json") or d.get("url"))
        detail = (f"b64 {len(d['b64_json'])} chars" if d.get("b64_json") else f"url {str(d.get('url'))[:60]}") if ok else _err(p)
        return {"model": mid, "kind": "image", "endpoint": "/v1/images/generations", "status": st,
                "ok": ok, "latency_s": round(dt, 2), "detail": detail}
    kind = "chat"
    st, p, dt = call("POST", "/v1/chat/completions", {
        "model": mid, "max_tokens": 16, "stream": False,
        "messages": [{"role": "user", "content": "Reply with the single word: pong"}]})
    ok = st == 200 and bool(p.get("choices"))
    reply, endpoint = "", "/v1/chat/completions"
    if ok:
        msg = p["choices"][0].get("message") or {}
        reply = (msg.get("content") or msg.get("reasoning_content") or "")[:60]
    else:  # some models (e.g. codex) are Responses-API only
        st2, p2, dt2 = call("POST", "/v1/responses", {
            "model": mid, "input": "Reply with the single word: pong", "max_output_tokens": 16})
        if st2 == 200:
            texts = [c.get("text", "") for o in p2.get("output", []) for c in (o.get("content") or [])]
            st, p, dt, ok, endpoint = st2, p2, dt2, True, "/v1/responses"
            reply = ("".join(texts) or p2.get("status", ""))[:60]
            kind = "responses"
    return {"model": mid, "kind": kind, "endpoint": endpoint, "status": st, "ok": ok,
            "latency_s": round(dt, 2), "detail": reply if ok else _err(p)}


def _err(p) -> str:
    if isinstance(p, dict):
        e = p.get("error", p)
        if isinstance(e, dict):
            e = e.get("message") or e
        return str(e)[:200]
    return str(p)[:200]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--only-list", action="store_true")
    ap.add_argument("--workers", type=int, default=8)
    args = ap.parse_args()

    report: dict = {"gateway": BASE, "ts": time.strftime("%Y-%m-%dT%H:%M:%S%z")}
    st, p, _ = call("GET", "/health")
    report["health"] = {"status": st, "body": p}
    st, p, dt = call("GET", "/v1/models")
    models = sorted(m["id"] for m in p.get("data", [])) if st == 200 else []
    report["models_list"] = {"status": st, "count": len(models), "latency_s": round(dt, 2),
                             "ids": models, "error": None if st == 200 else _err(p)}
    # auth check: a wrong local token must be rejected
    req = urllib.request.Request(BASE + "/v1/models", headers={"Authorization": "Bearer wrong"})
    try:
        urllib.request.urlopen(req, timeout=10)
        report["auth_reject"] = "FAILED (wrong token accepted)"
    except urllib.error.HTTPError as e:
        report["auth_reject"] = f"ok ({e.code})" if e.code == 401 else f"unexpected {e.code}"
    except Exception as e:
        report["auth_reject"] = f"gateway unreachable: {e}"

    if not args.only_list and models:
        with ThreadPoolExecutor(max_workers=args.workers) as ex:
            results = list(ex.map(test_model, models))
        report["calls"] = results
        report["summary"] = {"total": len(results), "ok": sum(r["ok"] for r in results),
                             "failed": [r["model"] for r in results if not r["ok"]]}
    out = HERE / "smoke_report.json"
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(report.get("summary") or report["models_list"], ensure_ascii=False)[:2000])


if __name__ == "__main__":
    main()
