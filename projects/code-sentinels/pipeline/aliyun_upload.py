"""Private, model-bound Aliyun uploads. Credentials never leave request headers.

Reference: https://help.aliyun.com/zh/model-studio/get-temporary-file-url
"""
import argparse
import base64
import ctypes
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "python-libs"))
import requests

ROOT = Path(__file__).resolve().parents[3]
BACKEND = "aliyun-minimax-video"
MODEL = "MiniMax/MiniMax-H3"


def credentials():
    doc = json.loads((ROOT / "data/keystore.json").read_text(encoding="utf-8"))
    if "dpapi" in doc:
        class Blob(ctypes.Structure):
            _fields_ = [("length", ctypes.c_ulong), ("data", ctypes.POINTER(ctypes.c_ubyte))]
        raw = base64.b64decode(doc["dpapi"])
        buf = (ctypes.c_ubyte * len(raw)).from_buffer_copy(raw)
        source, dest = Blob(len(raw), buf), Blob()
        if not ctypes.windll.crypt32.CryptUnprotectData(ctypes.byref(source), None, None, None, None, 0, ctypes.byref(dest)):
            raise RuntimeError("Cannot read current-user media credential")
        try:
            doc = json.loads(ctypes.string_at(dest.data, dest.length))
        finally:
            ctypes.windll.kernel32.LocalFree(dest.data)
    return doc["keys"][BACKEND]


def policy(endpoint="https://dashscope.aliyuncs.com"):
    response = requests.get(endpoint.rstrip("/") + "/api/v1/uploads", params={"action": "getPolicy", "model": MODEL},
                            headers={"Authorization": "Bearer " + credentials(), "Content-Type": "application/json"}, timeout=45)
    if response.status_code != 200:
        try:
            body = response.json()
        except ValueError:
            body = {}
        raise RuntimeError(f"Upload policy HTTP {response.status_code}: {body.get('code', '')} {body.get('message', '')}")
    return response.json()["data"]


def upload(path, endpoint="https://dashscope.aliyuncs.com"):
    path = Path(path).resolve(strict=True)
    if path.suffix.lower() not in {".png", ".jpg", ".jpeg", ".webp"}:
        raise ValueError("Only explicit reference image files are accepted")
    p = policy(endpoint)
    import uuid
    key = p["upload_dir"].rstrip("/") + "/" + uuid.uuid4().hex + path.suffix.lower()
    fields = {"OSSAccessKeyId": p["oss_access_key_id"], "Signature": p["signature"], "policy": p["policy"], "key": key,
              "x-oss-object-acl": p["x_oss_object_acl"], "x-oss-forbid-overwrite": p["x_oss_forbid_overwrite"], "success_action_status": "200"}
    with path.open("rb") as src:
        response = requests.post(p["upload_host"], data=fields, files={"file": (path.name, src)}, timeout=120)
    if response.status_code != 200:
        raise RuntimeError(f"Private image upload HTTP {response.status_code}")
    return {"uri": "oss://" + key, "file": str(path), "bytes": path.stat().st_size,
            "model": MODEL, "storage": "private-model-bound", "validHours": 48}


if __name__ == "__main__":
    args = argparse.ArgumentParser()
    args.add_argument("image", nargs="?")
    args.add_argument("--endpoint", default="https://dashscope.aliyuncs.com")
    opts = args.parse_args()
    try:
        result = upload(opts.image, opts.endpoint) if opts.image else policy(opts.endpoint)
        if not opts.image:
            result = {"available": True, "model": MODEL, "host": result["upload_host"], "acl": result["x_oss_object_acl"]}
        print(json.dumps(result, ensure_ascii=False))
    except Exception as error:
        # Do not print requests/headers, policy signatures, or any credential values.
        print(json.dumps({"error": str(error)}, ensure_ascii=False))
        sys.exit(1)
