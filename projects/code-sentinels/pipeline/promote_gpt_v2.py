"""Promote the verified margin-safe GPT video, preserving the complete v1 archive."""
import json
import shutil
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROJECT = HERE.parent
ROOT = PROJECT.parents[1]
read = lambda p: json.loads(p.read_text(encoding="utf-8-sig"))
write = lambda p, v: p.write_text(json.dumps(v, ensure_ascii=False, indent=2), encoding="utf-8")
target = PROJECT / "public/assets/characters"
data = read(target / "gpt-v2.json")
margins = read(HERE / "gpt-v2.margins.json")
receipt = read(HERE / "gpt-v2.video.json")
if not margins["passed"] or min(margins["minimumMarginsLTRB"]) < 10 or data["uniqueFrames"] != 32:
    raise ValueError("Refusing to promote a character that failed media quality requirements")
if not (PROJECT / "SourceMedia/gpt-v1.mp4").is_file() or not (HERE / "versions/gpt-v1/evidence.json").is_file():
    raise ValueError("Original version must be preserved before promotion")
video = PROJECT / receipt["artifacts"][0]["fileRef"]
shutil.copy2(video, PROJECT / "SourceMedia/gpt-v2.mp4")
shutil.copy2(video, PROJECT / "SourceMedia/gpt.mp4")
shutil.copy2(target / "gpt-v2.png", target / "gpt.png")
shutil.copy2(target / "gpt-v2-portrait.png", target / "gpt-portrait.png")
shutil.copy2(target / "gpt-v2-portrait.png", ROOT / "packages/client/public/games/code-sentinels/gpt.png")
data["id"], data["image"] = "gpt", "gpt.png"
data["qualityRevision"] = 2
data["provenance"]["temporaryVideoFileRef"] = receipt["artifacts"][0]["fileRef"]
data["provenance"]["videoFileRef"] = "SourceMedia/gpt.mp4"
data["marginValidation"] = {key:value for key,value in margins.items() if key != "frames"}
write(target / "gpt.json", data)
for suffix in ["request.json", "attempt.json", "video.json", "frames.json", "verification.json", "margins.json", "contact.png"]:
    shutil.copy2(HERE / f"gpt-v2.{suffix}", HERE / f"gpt.{suffix}")
verification = read(HERE / "gpt.verification.json")
verification["id"] = "gpt"
verification["videoFileRef"] = "SourceMedia/gpt.mp4"
verification["atlas"] = str(target / "gpt.png")
verification["uiPortrait"] = str(ROOT / "packages/client/public/games/code-sentinels/gpt.png")
verification["qualityRevision"] = 2
verification["minimumMarginsLTRB"] = margins["minimumMarginsLTRB"]
write(HERE / "gpt.verification.json", verification)
print(json.dumps({"promoted": "gpt-v2", "taskId": data["provenance"]["taskId"], "minimumMarginsLTRB": margins["minimumMarginsLTRB"], "allSourceFrames": margins["decodedFrames"]}))
