"""Run real image-to-video via RurixForge, then extract its sprite atlas.

Invoked by the project's Codex-mode agent. Receipts prevent silent paid retries.
"""
import argparse
import hashlib
import json
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROJECT = HERE.parent
ROOT = PROJECT.parents[1]
sys.path.insert(0, str(HERE / "python-libs"))
import requests
from PIL import Image
from aliyun_upload import upload

API = "http://127.0.0.1:8103/api/forge"
WORKSPACE = "ws_1788669812422_35134176"


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")


def call(route, payload, timeout=120):
    response = requests.post(API + route, json={"workspaceId": WORKSPACE, **payload}, timeout=timeout)
    result = response.json()
    if response.status_code != 200:
        raise RuntimeError(json.dumps(result, ensure_ascii=False))
    return result


def main(character):
    spec = json.loads((HERE / f"{character}.request.json").read_text(encoding="utf-8"))
    receipt_path = HERE / f"{character}.video.json"
    attempt_path = HERE / f"{character}.attempt.json"
    if receipt_path.exists():
        receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    else:
        if attempt_path.exists():
            raise RuntimeError("A paid generation was already submitted. Inspect its saved attempt/result; never resubmit automatically.")
        reference = spec.get("uploaded") or upload(PROJECT / spec["imageRef"])
        save(HERE / f"{character}.upload.private.json", reference)
        payload = {"backend": "aliyun-minimax-video", "prompt": spec["prompt"], "imageDataUrl": reference["uri"],
                   "aspect": "1:1", "resolution": "768p", "durationSec": 6}
        save(attempt_path, {"startedAt": datetime.now(timezone.utc).isoformat(), "imageRef": spec["imageRef"], "prompt": spec["prompt"], "mode": "image2video"})
        print(f"{character}: submitting real MiniMax-H3 image-to-video through RurixForge", flush=True)
        try:
            raw = call("/gen/video", payload, timeout=1050)
            receipt = {"backendId": raw["backendId"], "artifacts": [{k: v for k, v in item.items() if k not in {"dataUrl", "previews"}} for item in raw["artifacts"]]}
            save(receipt_path, receipt)
        except Exception as error:
            save(HERE / f"{character}.failure.json", {"error": str(error)})
            raise
    video = receipt["artifacts"][0]
    source_video = PROJECT / video["fileRef"]
    if not source_video.is_file() or video["meta"]["mode"] != "image2video":
        raise RuntimeError("Expected actual saved image-to-video output")
    print(f"{character}: provider video complete, extracting actual frames", flush=True)
    frames = call("/gen/video/frames", {"videoFileRef": video["fileRef"], "fps": 8, "maxFrames": 32,
                                        "chromaKey": "magenta", "crop": "union", "padding": 2}, timeout=180)
    save(HERE / f"{character}.frames.json", {k: v for k, v in frames.items() if k != "atlas"} | {"atlas": {k: v for k, v in frames["atlas"].items() if k != "dataUrl"}})
    target = PROJECT / "public/assets/characters"
    target.mkdir(parents=True, exist_ok=True)
    atlas = Image.open(PROJECT / frames["atlas"]["fileRef"]).convert("RGBA")
    boxes = frames["boxes"]
    unique = len({hashlib.sha256(atlas.crop((x, y, x+w, y+h)).tobytes()).hexdigest() for x, y, w, h in boxes})
    if len(boxes) != 32 or unique < 16 or len({(w, h) for x, y, w, h in boxes}) != 1:
        raise RuntimeError(f"Animation evidence failed: frames={len(boxes)}, distinct={unique}")
    shutil.copy2(PROJECT / frames["atlas"]["fileRef"], target / f"{character}.png")
    x, y, w, h = boxes[0]
    portrait = atlas.crop((x, y, x+w, y+h))
    portrait.save(target / f"{character}-portrait.png")
    ui = ROOT / "packages/client/public/games/code-sentinels"
    if ui.is_relative_to(PROJECT):
        ui.mkdir(parents=True, exist_ok=True)
        portrait.save(ui / f"{character}.png")
    save(target / f"{character}.json", {"id": character, "image": f"{character}.png", "width": atlas.width, "height": atlas.height,
                                         "fps": frames["fps"], "frameCount": len(boxes), "boxes": boxes, "frames": boxes,
                                         "pivot": [0.5, 1.0], "crop": "union", "chromaKey": "magenta", "uniqueFrames": unique,
                                         "provenance": {"method": "image-to-video-extracted-frames", "backend": receipt["backendId"], "taskId": video["meta"]["taskId"],
                                                        "videoFileRef": video["fileRef"], "videoSha256": hashlib.sha256(source_video.read_bytes()).hexdigest(), "sourceImage": spec["imageRef"]}})
    print(json.dumps({"character": character, "taskId": video["meta"]["taskId"], "video": str(source_video), "atlas": str(target / f"{character}.png"), "frames": len(boxes), "uniqueFrames": unique}, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("character", choices=["deepseek", "gpt"])
    main(parser.parse_args().character)
