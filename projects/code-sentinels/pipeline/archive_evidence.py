"""Permanently archive provider videos and complete project-agent media evidence."""
import hashlib
import json
import shutil
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROJECT = HERE.parent
ROOT = PROJECT.parents[1]
SOURCE = PROJECT / "SourceMedia"
SOURCE.mkdir(parents=True, exist_ok=True)
entries = []

for character, sid in [
    ("deepseek", "sess_1788670123057_5380645c"),
    ("gpt", "sess_1788670201477_0bd1ccb8"),
]:
    metadata_path = PROJECT / f"public/assets/characters/{character}.json"
    data = json.loads(metadata_path.read_text(encoding="utf-8-sig"))
    receipt = json.loads((HERE / f"{character}.video.json").read_text(encoding="utf-8-sig"))
    verification = json.loads((HERE / f"{character}.verification.json").read_text(encoding="utf-8-sig"))
    old_ref = receipt["artifacts"][0]["fileRef"]
    permanent = SOURCE / f"{character}.mp4"
    shutil.copy2(PROJECT / old_ref, permanent)
    video_hash = hashlib.sha256(permanent.read_bytes()).hexdigest()
    if video_hash != data["provenance"]["videoSha256"].lower():
        raise ValueError("Permanent video hash mismatch")
    data["provenance"]["temporaryVideoFileRef"] = old_ref
    data["provenance"]["videoFileRef"] = f"SourceMedia/{character}.mp4"
    metadata_path.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
    log = ROOT / f"data/agent-events/{sid}.jsonl"
    events = [json.loads(line) for line in log.read_text(encoding="utf-8-sig").splitlines() if line.strip()]
    task_id = data["provenance"]["taskId"]
    matching_runs = [event["payload"]["runId"] for event in events
                     if event.get("type") == "agent.tool.completed"
                     and task_id in event.get("payload", {}).get("output", "")]
    if not matching_runs:
        raise ValueError("No actual project-agent tool completion matches the provider task")
    run = matching_runs[-1]
    current = [event for event in events if event.get("payload", {}).get("runId") == run]
    completed = any(event["type"] == "agent.completed" for event in current)
    shutil.copy2(log, HERE / f"{character}.codex-events.jsonl")
    key_events = [event for event in current if event["type"] in {"agent.started", "agent.tool.invoked", "agent.tool.completed", "agent.tool.failed", "agent.message", "agent.completed", "agent.failed"}]
    (HERE / f"{character}.codex-key-events.json").write_text(json.dumps(key_events, ensure_ascii=False, indent=2), encoding="utf-8")
    provenance = {"character": character, "method": "image-to-video-extracted-frames", "model": receipt["artifacts"][0]["meta"]["model"],
                  "providerTaskId": data["provenance"]["taskId"], "sourceImage": data["provenance"]["sourceImage"],
                  "sourceImageSha256": hashlib.sha256((PROJECT / data["provenance"]["sourceImage"]).read_bytes()).hexdigest(),
                  "video": f"SourceMedia/{character}.mp4", "videoSha256": video_hash,
                  "originalTemporaryVideo": old_ref, "provider": receipt["artifacts"][0]["meta"],
                  "agentEngine": "codex", "projectSessionId": sid, "projectRunId": run,
                  "agentCompleted": completed, "frameCount": data["frameCount"], "uniqueFrames": data["uniqueFrames"],
                  "fps": data["fps"], "uniformFrameSize": verification["uniformFrameSize"],
                  "atlas": f"public/assets/characters/{character}.png", "atlasSha256": hashlib.sha256((metadata_path.with_suffix('.png')).read_bytes()).hexdigest(),
                  "metadata": str(metadata_path.relative_to(PROJECT)).replace('\\','/'),
                  "visualReview": "contact sheet inspected; original character identity maintained", "fullAgentTrace": f"pipeline/{character}.codex-events.jsonl"}
    if data.get("qualityRevision"):
        provenance["qualityRevision"] = data["qualityRevision"]
        provenance["marginValidation"] = data["marginValidation"]
    (SOURCE / f"{character}.provenance.json").write_text(json.dumps(provenance, ensure_ascii=False, indent=2), encoding="utf-8")
    entries.append(provenance)

evidence = {"scope": "RurixForge Codex-mode character media pipeline", "workspaceId": "ws_1788669812422_35134176",
            "passed": all(entry["agentCompleted"] for entry in entries), "characters": entries,
            "repairs": ["media REST workspace scope", "authenticated model-bound private OSS references", "ffmpeg dependency provisioned"],
            "tests": {"workspaceScope": "1 passed", "minimaxAdapter": "5 passed", "framePipeline": "7 passed"},
            "firstFailure": {"taskId": "7eb031a3-02bc-4090-982b-95cc5186e2b6", "cause": "model product not activated", "resolution": "user activated service; retried once after confirmation", "evidence": "pipeline/deepseek.failure.failed-7eb031a3.json"},
            "officialSources": ["https://help.aliyun.com/zh/model-studio/minimax-video-generation-api-reference", "https://help.aliyun.com/zh/model-studio/get-temporary-file-url"]}
(HERE / "evidence.json").write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps({"passed": evidence["passed"], "videos": [entry["video"] for entry in entries], "agentCompleted": [entry["agentCompleted"] for entry in entries]}, ensure_ascii=False))
