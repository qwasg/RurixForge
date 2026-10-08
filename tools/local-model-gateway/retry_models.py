"""Retry specific models via gateway AND direct upstream. Usage: python retry_models.py m1 m2 ..."""
import json
import os
import sys
import urllib.error
import urllib.request

import smoke_test as st  # loads .env, exposes call()/test_model()

UP = os.environ.get("UPSTREAM_BASE", "https://sg-api.kcne.top").rstrip("/")
KEY = os.environ["UPSTREAM_API_KEY"]


def direct(mid: str):
    if "image" in mid:
        path, body = "/v1/images/generations", {"model": mid, "prompt": "a red circle", "n": 1,
                                                "size": "1024x1024", "quality": "low"}
    else:
        path, body = "/v1/chat/completions", {"model": mid, "max_tokens": 16,
                                              "messages": [{"role": "user", "content": "Reply: pong"}]}
    req = urllib.request.Request(UP + path, data=json.dumps(body).encode(), method="POST",
                                 headers={"Authorization": f"Bearer {KEY}", "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=300) as r:
            return r.status, "ok"
    except urllib.error.HTTPError as e:
        return e.code, e.read()[:200].decode("utf-8", "replace")
    except Exception as e:
        return 0, f"{e.__class__.__name__}: {e}"


for m in sys.argv[1:]:
    for i in range(2):
        g = st.test_model(m)
        print(f"[gateway try{i+1}] {m}: {g['status']} ok={g['ok']} {str(g['detail'])[:120]}")
    print(f"[direct] {m}: {direct(m)}")
