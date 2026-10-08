"""Inspect EVERY decoded source-video frame using gend's magenta-key rule."""
import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROJECT = HERE.parent
sys.path.insert(0, str(HERE / "python-libs"))
import imageio_ffmpeg
from PIL import Image, ImageMath


def verify(character):
    receipt = json.loads((HERE / f"{character}.video.json").read_text(encoding="utf-8-sig"))
    path = PROJECT / receipt["artifacts"][0]["fileRef"]
    stream = imageio_ffmpeg.read_frames(str(path), pix_fmt="rgb24")
    info = next(stream)
    width, height = info["size"]
    frames = []
    minima = [width, height, width, height]
    for index, raw in enumerate(stream):
        image = Image.frombytes("RGB", (width, height), raw)
        red, green, blue = image.split()
        # gend::video_frames magenta key: remove where 2*g < min(r,b).
        foreground = ImageMath.lambda_eval(lambda args: (args["g"]*2 >= args["r"]) | (args["g"]*2 >= args["b"]), r=red, g=green, b=blue).convert("L")
        box = foreground.getbbox()
        if box is None:
            raise ValueError(f"Frame {index} has no character foreground")
        left, top, right, bottom = box
        margins = [left, top, width-right, height-bottom]
        minima = [min(old, new) for old, new in zip(minima, margins)]
        frames.append({"index": index, "bbox": list(box), "margins": margins})
    result = {"character": character, "videoFileRef": receipt["artifacts"][0]["fileRef"], "providerTaskId": receipt["artifacts"][0]["meta"]["taskId"],
              "decodedFrames": len(frames), "sourceFps": info["fps"], "duration": info["duration"], "size": [width,height],
              "requiredMinimumPixels": 10, "minimumMarginsLTRB": minima, "passed": min(minima) >= 10, "frames": frames}
    (HERE / f"{character}.margins.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({key:value for key,value in result.items() if key != "frames"}, ensure_ascii=False))
    if not result["passed"]:
        raise ValueError("Source video does not maintain the required 10-pixel foreground margin")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("character", choices=["gpt-v2", "gpt", "deepseek"])
    verify(parser.parse_args().character)
