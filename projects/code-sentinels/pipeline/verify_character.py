"""Verify real generated atlas pixels, preserve provenance and export first-frame UI portrait."""
import argparse
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROJECT = HERE.parent
ROOT = PROJECT.parents[1]
sys.path.insert(0, str(HERE / "python-libs"))
from PIL import Image, ImageDraw, ImageChops, ImageStat


def verify(character):
    target = PROJECT / "public/assets/characters"
    path = target / f"{character}.json"
    data = json.loads(path.read_text(encoding="utf-8-sig"))
    atlas = Image.open(target / data["image"]).convert("RGBA")
    frames = [atlas.crop((x, y, x+w, y+h)) for x, y, w, h in data["boxes"]]
    hashes = {hashlib.sha256(frame.tobytes()).hexdigest() for frame in frames}
    dimensions = {frame.size for frame in frames}
    source = PROJECT / data["provenance"]["videoFileRef"]
    source_hash = hashlib.sha256(source.read_bytes()).hexdigest()
    if source_hash.lower() != data["provenance"]["videoSha256"].lower():
        raise ValueError("Video changed since extraction")
    if len(frames) != 32 or len(hashes) < 16 or len(dimensions) != 1:
        raise ValueError("Expected 32 genuinely distinct equal-size extracted frames")
    alpha_hist = frames[0].getchannel("A").histogram()
    transparent_fraction = alpha_hist[0] / (frames[0].width * frames[0].height)
    if transparent_fraction < 0.05 or transparent_fraction > 0.99:
        raise ValueError("Unexpected chroma key result")
    data["uniqueFrames"] = len(hashes)
    data["transparentFraction"] = round(transparent_fraction, 4)
    data["provenance"]["videoSha256"] = source_hash
    data["verification"] = {"actualVideo": True, "uniformFrameSize": list(frames[0].size), "frameHashesUnique": len(hashes),
                            "meanPixelChange": [round(sum(ImageStat.Stat(ImageChops.difference(frames[i-1], frames[i])).mean[:3])/3, 3) for i in range(1, len(frames))]}
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
    portrait = frames[0]
    portrait.save(target / f"{character}-portrait.png")
    ui = ROOT / "packages/client/public/games/code-sentinels"
    ui.mkdir(parents=True, exist_ok=True)
    portrait.save(ui / f"{character}.png")
    contact = Image.new("RGB", (1024, 592), "#202536")
    draw = ImageDraw.Draw(contact)
    for panel, frame_id in enumerate([0, 4, 8, 12, 16, 20, 24, 31]):
        x, y = (panel % 4) * 256, (panel // 4) * 296
        for cy in range(y, y+272, 16):
            for cx in range(x, x+256, 16):
                color = "#343c50" if ((cx-x)//16+(cy-y)//16)%2 else "#272f41"
                draw.rectangle((cx, cy, cx+15, cy+15), fill=color)
        frame = frames[frame_id].copy()
        frame.thumbnail((244, 260), Image.Resampling.LANCZOS)
        contact.paste(frame, (x+(256-frame.width)//2, y+(272-frame.height)//2), frame)
        draw.text((x+12, y+278), f"actual video frame {frame_id:02d}", fill="#d1ddf3")
    contact.save(HERE / f"{character}.contact.png")
    evidence = {"id": character, "frameCount": len(frames), "uniqueFrames": len(hashes), "uniformFrameSize": list(frames[0].size),
                "transparentFraction": data["transparentFraction"], "taskId": data["provenance"]["taskId"],
                "videoFileRef": data["provenance"]["videoFileRef"], "videoSha256": source_hash,
                "atlas": str(target / data["image"]), "uiPortrait": str(ui / f"{character}.png")}
    (HERE / f"{character}.verification.json").write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(evidence, ensure_ascii=False))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("character", choices=["deepseek", "gpt", "gpt-v2"])
    verify(parser.parse_args().character)
