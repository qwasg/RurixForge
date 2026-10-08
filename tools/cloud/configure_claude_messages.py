"""Configure the local Claude Messages channel without printing/storing credentials.

Reads the existing encrypted channel key and local administrator credential file.
Admin APIs record mutations; a credential-free configuration backup is appended to evidence.
"""
import base64
import json
import subprocess
import urllib.request
import uuid
from datetime import datetime, timezone
from pathlib import Path

from cryptography.hazmat.primitives.ciphers.aead import AESGCM

ROOT = Path(__file__).resolve().parents[2]
ORIGIN = "http://127.0.0.1:8110"
MODEL_IDS = ["claude-sonnet-5-5", "claude-opus-5-5", "claude-fable-5-1", "claude-haiku-4-5"]


def api(method, path, body=None, token=None):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = "Bearer " + token
    request = urllib.request.Request(
        ORIGIN + path, data=json.dumps(body).encode() if body is not None else None,
        headers=headers, method=method,
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def main():
    secret = json.loads((ROOT / "data/cloud-server/credentials.json").read_text(encoding="utf-8-sig"))
    login = api("POST", "/api/v1/auth/login", {
        "email": secret["adminEmail"], "password": secret["adminPassword"],
        "device": {"id": "claude-config-" + str(uuid.uuid4()), "name": "Claude Messages configuration",
                   "platform": "windows", "appVersion": "0.1.0"}, "issueDeviceKey": False,
    })
    token = login["accessToken"]
    try:
        accounts = api("GET", "/api/admin/accounts", token=token)["items"]
        old = next(a for a in accounts if a["id"] == 1)
        models = [m for m in api("GET", "/api/admin/models", token=token)["items"] if m["id"] in MODEL_IDS]
        assert len(models) == 4
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        evidence = ROOT / "evidence/cloud-server-20261007"
        evidence.mkdir(exist_ok=True)
        fields = ["id", "name", "platform", "authType", "baseUrl", "status", "priority", "weight",
                  "concurrencyLimit", "groupIds", "modelMapping", "allowedModels", "supportsResponses", "proxyUrl"]
        before = {"createdAtUTC": stamp, "account": {k: old.get(k) for k in fields}, "models": models}
        (evidence / f"claude-messages-before-{stamp}.json").write_text(
            json.dumps(before, ensure_ascii=False, indent=2), encoding="utf-8")
        candidates = [a for a in accounts if a["name"] == "KCNE · Claude 最新 · Messages" and a["platform"] == "anthropic"]
        if candidates:
            new = candidates[0]
        else:
            env = dict(line.split("=", 1) for line in (ROOT / "cloud/deploy/.env").read_text().splitlines()
                       if "=" in line and not line.lstrip().startswith("#"))
            master = env["FORGE_CLOUD_MASTER_KEY"].strip().strip('"').strip("'")
            key = bytes.fromhex(master) if len(master) == 64 else base64.b64decode(master)
            value = subprocess.check_output([
                "docker", "compose", "-f", "cloud/deploy/docker-compose.yml", "exec", "-T", "postgres",
                "psql", "-U", "forge", "-d", "forge_cloud", "-At", "-c",
                "SELECT encode(credentials,'hex') FROM upstream_accounts WHERE id=1;",
            ], cwd=ROOT, text=True).strip()
            blob = bytes.fromhex(value)
            api_key = json.loads(AESGCM(key).decrypt(blob[1:13], blob[13:], blob[:1]))["apiKey"]
            params = {k: old.get(k) for k in ["baseUrl", "priority", "weight", "concurrencyLimit",
                                             "groupIds", "modelMapping", "allowedModels", "proxyUrl"]}
            params.update({"name": "KCNE · Claude 最新 · Messages", "platform": "anthropic",
                           "apiKey": api_key, "supportsResponses": False})
            new = api("POST", "/api/admin/accounts", params, token)
        for model in models:
            cap = dict(model["capabilities"])
            manual = model["id"] == "claude-haiku-4-5"
            always = model["id"] in ["claude-opus-5-5", "claude-fable-5-1"]
            cap.update({"thinkingMode": "manual" if manual else "adaptive", "thinkingAlwaysOn": always,
                        "responses": False, "tools": True})
            cap["reasoningEfforts"] = (["low", "medium", "high"] if manual else
                (["high", "low", "medium", "xhigh", "max"] if model["id"] == "claude-fable-5-1" else
                 ["medium", "low", "high", "xhigh", "max"]))
            api("PATCH", "/api/admin/models/" + model["id"], {"platform": "anthropic", "capabilities": cap}, token)
        api("PATCH", "/api/admin/accounts/1", {"status": "disabled"}, token)
        after = api("GET", "/api/admin/models", token=token)["items"]
        result = {"createdAtUTC": stamp, "activeAccount": {k: new.get(k) for k in fields},
                  "previousAccountStatus": "disabled", "models": [m for m in after if m["id"] in MODEL_IDS]}
        (evidence / f"claude-messages-configured-{stamp}.json").write_text(
            json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
        print(json.dumps({"activeAccountId": new["id"], "platform": new["platform"],
                          "models": [{k: m[k] for k in ["id", "platform", "capabilities", "availableAccounts"]}
                                     for m in result["models"]]}, ensure_ascii=False))
    finally:
        api("POST", "/api/v1/auth/logout", {}, token)


if __name__ == "__main__":
    main()
