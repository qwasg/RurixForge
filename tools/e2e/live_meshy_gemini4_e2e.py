#!/usr/bin/env python3
"""Real Live End-to-End Test for RurixForge 3D Character Pipeline.

Uses:
  1. Real Antigravity reverse-proxy channel (`antigravity:gemini-4`) via `forge-agentd`
     and `forge-antigravity-bridge.exe`
  2. Real Meshy 3D API (`https://api.meshy.ai/openapi/v1/image-to-3d` / `text-to-3d`)
     via `gen-model-mcp`
  3. Real 5-stage pipeline tools:
     - Stage 1: Concept Art (`Content/ConceptArt/hero_apose.png`) -> Gate 1
     - Stage 2: `mcp__gen-model__gen_mesh` (Meshy API) -> Gate 2
     - Stage 3: `mcp__gen-model__gen_mesh_refine` -> Gate 3
     - Stage 4: `mcp__gen-model__gen_mesh_rig` -> Gate 4
     - Stage 5: `mcp__gen-model__gen_mesh_animate` + `mcp__gen-model__gen_character_publish` -> Gate 5
  4. Real Engine Scene Instantiation & Viewport Frame Capture (`prefab_instantiate` + `viewport_frame`)
"""

import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request

from PIL import Image, ImageDraw

WORKSPACE_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(WORKSPACE_ROOT / "tools" / "e2e"))

from character_3d_pipeline import (
    validate_gate_1_concept_art,
    validate_gate_2_raw_glb,
    validate_gate_3_polycount,
    validate_gate_4_rigging,
    validate_gate_5_motion_publish,
)


def create_apose_concept_art(out_path: Path) -> None:
    """Draws a clear 512x512 RGBA A-Pose Stylized Armored Knight Concept Art."""
    out_path.parent.mkdir(parents=True, exist_ok=True)
    img = Image.new("RGBA", (512, 512), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    # Head & Helmet
    draw.ellipse([216, 45, 296, 125], fill=(60, 130, 210, 255), outline=(220, 240, 255, 255), width=4)
    # Visor glow
    draw.rectangle([232, 78, 280, 92], fill=(0, 255, 230, 255))
    # Neck
    draw.rectangle([242, 125, 270, 145], fill=(80, 90, 110, 255))
    # Torso / Chestplate
    draw.polygon(
        [(195, 145), (317, 145), (302, 285), (210, 285)],
        fill=(45, 95, 175, 255),
        outline=(200, 220, 245, 255),
    )
    # Chest core emblem
    draw.ellipse([240, 185, 272, 217], fill=(0, 240, 255, 255))
    # Belt / Pelvis
    draw.rectangle([210, 285, 302, 315], fill=(70, 75, 90, 255), outline=(180, 190, 210, 255), width=2)
    # Left Arm (A-pose ~40 deg)
    draw.polygon([(195, 150), (165, 165), (105, 275), (135, 290)], fill=(55, 110, 190, 255), outline=(190, 215, 240, 255))
    # Right Arm (A-pose ~40 deg)
    draw.polygon([(317, 150), (347, 165), (407, 275), (377, 290)], fill=(55, 110, 190, 255), outline=(190, 215, 240, 255))
    # Left Leg
    draw.polygon([(212, 315), (250, 315), (242, 470), (198, 470)], fill=(40, 75, 145, 255), outline=(180, 205, 235, 255))
    # Right Leg
    draw.polygon([(262, 315), (300, 315), (314, 470), (270, 470)], fill=(40, 75, 145, 255), outline=(180, 205, 235, 255))

    img.save(out_path, format="PNG")


def get_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def http_json(method: str, url: str, payload=None, timeout: int = 1800, headers=None):
    req_headers = {"Content-Type": "application/json"}
    if headers:
        req_headers.update(headers)
    data = json.dumps(payload).encode("utf-8") if payload is not None else None
    req = urllib.request.Request(url, data=data, method=method, headers=req_headers)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open(req, timeout=timeout) as resp:
        return resp.status, json.loads(resp.read().decode("utf-8"))


def load_meshy_key() -> str:
    env_k = os.environ.get("MESHY_API_KEY", "").strip()
    if env_k:
        return env_k
    ks_path = WORKSPACE_ROOT / "data" / "keystore.json"
    if ks_path.is_file():
        ks = json.loads(ks_path.read_text(encoding="utf-8"))
        if "dpapi" in ks and sys.platform == "win32":
            import base64
            import ctypes
            import ctypes.wintypes

            class DATA_BLOB(ctypes.Structure):
                _fields_ = [
                    ("cbData", ctypes.wintypes.DWORD),
                    ("pbData", ctypes.POINTER(ctypes.c_char)),
                ]

            cipher = base64.b64decode(ks["dpapi"])
            buf = ctypes.create_string_buffer(cipher, len(cipher))
            blob_in = DATA_BLOB(len(cipher), buf)
            blob_out = DATA_BLOB()
            if ctypes.windll.crypt32.CryptUnprotectData(
                ctypes.byref(blob_in), None, None, None, None, 0, ctypes.byref(blob_out)
            ):
                plain = ctypes.string_at(blob_out.pbData, blob_out.cbData).decode("utf-8")
                ctypes.windll.kernel32.LocalFree(blob_out.pbData)
                ks = json.loads(plain)
        val = ks.get("keys", {}).get("meshy", "")
        if isinstance(val, str) and val.strip():
            return val.strip()
        if isinstance(val, dict):
            return val.get("secret", val.get("key", "")).strip()
    raise RuntimeError("Meshy API key not found in MESHY_API_KEY or data/keystore.json")


def check_meshy_balance(api_key: str) -> dict:
    req = urllib.request.Request(
        "https://api.meshy.ai/openapi/v1/balance",
        headers={"Authorization": f"Bearer {api_key}"},
    )
    with urllib.request.urlopen(req, timeout=20) as resp:
        return json.loads(resp.read().decode("utf-8"))


def run_live_e2e() -> dict:
    start_ts = time.time()
    artifacts_dir = WORKSPACE_ROOT / ".forge" / "tmp" / "e2e" / "live_artifacts"
    artifacts_dir.mkdir(parents=True, exist_ok=True)

    demo_root = WORKSPACE_ROOT / "projects" / "demo"
    # Clean up previous published model directory so revision=1 succeeds cleanly
    for root_dir in (WORKSPACE_ROOT, demo_root):
        prev_models = root_dir / "Content" / "Models" / "f07bda00-c937-5324-ae5e-cd625c8a1e72"
        if prev_models.exists():
            shutil.rmtree(prev_models, ignore_errors=True)
        prev_meta = root_dir / "Content" / "Models" / "f07bda00-c937-5324-ae5e-cd625c8a1e72.meta"
        if prev_meta.exists():
            prev_meta.unlink(missing_ok=True)

    concept_rel = "Content/ConceptArt/hero_apose.png"
    concept_abs = WORKSPACE_ROOT / concept_rel
    create_apose_concept_art(concept_abs)
    create_apose_concept_art(demo_root / concept_rel)
    shutil.copy2(concept_abs, artifacts_dir / "stage1_hero_apose.png")

    # Validate Gate 1
    g1 = validate_gate_1_concept_art(concept_abs)
    assert g1.passed, f"Gate 1 failed: {g1.message}"
    print(f"[Gate 1 PASS] {concept_rel}: {g1.details}", flush=True)

    meshy_key = load_meshy_key()
    balance_before = check_meshy_balance(meshy_key)
    print(f"[Meshy] Balance before test: {balance_before}", flush=True)

    # Ensure data/gen-backends.json has meshy enabled
    gen_backends_path = WORKSPACE_ROOT / "data" / "gen-backends.json"
    gen_backends_path.parent.mkdir(parents=True, exist_ok=True)
    if gen_backends_path.is_file():
        gb = json.loads(gen_backends_path.read_text(encoding="utf-8"))
    else:
        gb = {"backends": []}
    found_meshy = False
    for b in gb.get("backends", []):
        if b.get("id") == "meshy":
            b["kind"] = "remote"
            b["enabled"] = True
            found_meshy = True
    if not found_meshy:
        gb.setdefault("backends", []).append(
            {
                "id": "meshy",
                "kind": "remote",
                "model": "latest",
                "enabled": True,
            }
        )
    gen_backends_path.write_text(json.dumps(gb, indent=2, ensure_ascii=False), encoding="utf-8")

    # Spawn real forge-agentd.exe
    port = get_free_port()
    agentd_bin = WORKSPACE_ROOT / "target" / "debug" / "forge-agentd.exe"
    if not agentd_bin.is_file():
        raise FileNotFoundError(f"Missing binary: {agentd_bin}")

    env = os.environ.copy()
    env["FORGE_AGENTD_ADDR"] = f"127.0.0.1:{port}"
    env["MESHY_API_KEY"] = meshy_key
    # Ensure local proxy is passed if 127.0.0.1:7897 is listening
    try:
        with socket.create_connection(("127.0.0.1", 7897), timeout=0.1):
            env["HTTP_PROXY"] = "http://127.0.0.1:7897"
            env["HTTPS_PROXY"] = "http://127.0.0.1:7897"
            env["NO_PROXY"] = "127.0.0.1,localhost,api.meshy.ai,assets.meshy.ai"
    except OSError:
        pass

    # Temporarily set forge.toml mode = "3d" so currentProjectMode is 3D during E2E
    forge_toml_path = WORKSPACE_ROOT / "forge.toml"
    orig_forge_toml = forge_toml_path.read_text(encoding="utf-8") if forge_toml_path.is_file() else None
    if orig_forge_toml:
        forge_toml_path.write_text(orig_forge_toml.replace('mode = "2d"', 'mode = "3d"'), encoding="utf-8")

    base_url = f"http://127.0.0.1:{port}"
    print(f"[forge-agentd] Starting on {base_url} ...", flush=True)
    proc = subprocess.Popen(
        [str(agentd_bin)],
        cwd=str(WORKSPACE_ROOT),
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    def resolve_proj_path(rel_path: str) -> Path:
        p1 = WORKSPACE_ROOT / rel_path
        if p1.exists():
            return p1
        p2 = demo_root / rel_path
        if p2.exists():
            return p2
        return p1

    try:
        # Wait for health check
        for _ in range(60):
            try:
                status, _ = http_json("GET", f"{base_url}/health", timeout=5)
                if status == 200:
                    break
            except Exception:
                time.sleep(0.5)
        else:
            raise RuntimeError("forge-agentd failed to become healthy")

        # 1. Verify Antigravity channel & Gemini 4 registration
        status, ag_channel = http_json(
            "GET", f"{base_url}/api/forge/channels/antigravity?refresh=true", timeout=30
        )
        print(f"[Antigravity Channel] status={status}, configured={ag_channel.get('configured')}, account={ag_channel.get('account')}, models={[m.get('id') for m in ag_channel.get('models', [])]}", flush=True)
        assert ag_channel.get("configured") is True, f"Antigravity channel not configured: {ag_channel}"

        # 2. Create a real Agent session using `antigravity:gemini-4`
        _, sess_resp = http_json(
            "POST",
            f"{base_url}/api/forge/sessions",
            {
                "title": "Live E2E 3D Character Pipeline (Meshy + Gemini 4)",
                "selectedModelId": "antigravity:gemini-4",
                "thinkingEnabled": True,
                "reasoningEffort": "medium",
            },
        )
        session = sess_resp.get("session", sess_resp)
        session_id = session["id"]

        # Ensure agentEngine is local and model is antigravity:gemini-4
        _, patched = http_json(
            "PATCH",
            f"{base_url}/api/forge/sessions/{session_id}",
            {
                "selectedModelId": "antigravity:gemini-4",
                "agentEngine": "local",
                "thinkingEnabled": True,
                "reasoningEffort": "medium",
            },
        )
        print(f"[Session Created] id={session_id}, model={patched.get('session', patched).get('selectedModelId')}", flush=True)

        # 3. Execute the 3D character production pipeline via Antigravity Gemini 4 Agent!
        user_prompt = (
            "请严格按照 character-3d-pipeline 技能规程，在当前项目中执行完整的 3D 角色制作并发布（不要跳过任何一步，依次调用工具）：\n"
            "1. 调用 `mcp__gen-model__gen_mesh`：传入 `imageRef: \"Content/ConceptArt/hero_apose.png\"`, `prompt: \"stylized cyber knight humanoid character in A-pose, game ready\"`, `poseMode: \"a-pose\"`, `targetPolyBudget: 8000`, `texture: true`，生成初始 3D 角色 GLB。\n"
            "2. 拿到第 1 步返回的 `candidates[0].meshFileRef` 后，立即调用 `mcp__gen-model__gen_mesh_refine`：传入该 `meshFileRef`、`targetFaces: 4000`、`ops: {\"retopo\": {\"targetFaces\": 4000}}` 进行减面精修。\n"
            "3. 拿到第 2 步返回的 `refinedMeshFileRef` 后，立即调用 `mcp__gen-model__gen_mesh_rig`：传入 `meshFileRef: <refinedMeshFileRef>`, `heightMeters: 1.7` 绑定标准人形骨骼。\n"
            "4. 拿到第 3 步返回的 `riggedMeshFileRef` 后，立即调用 `mcp__gen-model__gen_mesh_animate`：传入 `meshFileRef: <riggedMeshFileRef>` 合入标准人形动作包（Idle/Walk/Run/Attack）。\n"
            "5. 拿到第 4 步返回的 `animatedMeshFileRef` 后，立即调用 `mcp__gen-model__gen_character_publish`：传入 `meshFileRef: <animatedMeshFileRef>`, `name: \"meshy_cyber_knight\"`, `category: \"role\"` 发布正式 `.rxmodel` 与 `template.rxprefab`。\n"
            "6. 最后调用 `mcp__engine-scene__prefab_instantiate`（传入 `prefabPath: <rxprefabUri>`, `name: \"MeshyCyberKnight\"`）将角色放入场景，并调用 `mcp__engine-scene__viewport_frame` 截图验证。"
        )

        print("[Agent Turn 1] Sending prompt to Antigravity Gemini 4 Agent...", flush=True)
        _, turn1_resp = http_json(
            "POST",
            f"{base_url}/api/forge/sessions/{session_id}/ask:execute",
            {
                "userInput": user_prompt,
                "mode": "build",
                "skills": ["character-3d-pipeline"],
            },
            timeout=1800,
        )
        print(f"[Agent Turn 1 Completed] status={turn1_resp.get('status')}", flush=True)

        # Inspect session events to see what tools were called
        events_file = WORKSPACE_ROOT / "data" / "agent-events" / f"{session_id}.jsonl"
        events = [
            json.loads(line)
            for line in events_file.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]
        called_tools = [
            e["payload"].get("name")
            for e in events
            if e.get("type") in ("agent.tool.invoked", "agent.tool.completed") and e["payload"].get("name")
        ]
        print(f"[Agent Turn 1 Tools Invoked] {called_tools}", flush=True)

        # Extract tool results from session events helper
        def extract_tool_outputs(evts):
            outputs = {}
            for e in evts:
                if e.get("type") == "agent.tool.completed":
                    name = e["payload"].get("name")
                    out_val = e["payload"].get("output", "")
                    if isinstance(out_val, dict):
                        if "content" in out_val and isinstance(out_val["content"], list) and out_val["content"]:
                            txt = out_val["content"][0].get("text", "")
                            try:
                                out_val = json.loads(txt)
                            except Exception:
                                pass
                        outputs[name] = out_val
                    elif isinstance(out_val, str):
                        try:
                            out_json = json.loads(out_val)
                            if isinstance(out_json, dict) and "content" in out_json:
                                txt = out_json["content"][0].get("text", "")
                                out_json = json.loads(txt)
                            outputs[name] = out_json
                        except Exception:
                            outputs[name] = out_val
            return outputs

        tool_outputs = extract_tool_outputs(events)

        # If the agent stopped before completing all steps, send a continuation turn
        required_tools = [
            "mcp__gen-model__gen_mesh",
            "mcp__gen-model__gen_mesh_refine",
            "mcp__gen-model__gen_mesh_rig",
            "mcp__gen-model__gen_mesh_animate",
            "mcp__gen-model__gen_character_publish",
        ]
        for retry in range(4):
            missing = [t for t in required_tools if t not in tool_outputs or (isinstance(tool_outputs[t], dict) and tool_outputs[t].get("isError"))]
            if not missing:
                break
            print(f"[Agent Continuation {retry+1}] Missing/incomplete tools: {missing}, prompting agent to continue...", flush=True)
            _, cont_resp = http_json(
                "POST",
                f"{base_url}/api/forge/sessions/{session_id}/ask:execute",
                {
                    "userInput": (
                        f"请继续依次调用剩余未完成的工具步骤：{', '.join(missing)}，以及 `mcp__engine-scene__prefab_instantiate` 和 `mcp__engine-scene__viewport_frame`。"
                        f"目前已完成的工具输出：{json.dumps({k: v for k, v in tool_outputs.items() if k in required_tools}, ensure_ascii=False)}"
                    ),
                    "mode": "build",
                    "skills": ["character-3d-pipeline"],
                },
                timeout=1800,
            )
            events = [
                json.loads(line)
                for line in events_file.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
            called_tools = [
                e["payload"].get("name")
                for e in events
                if e.get("type") == "agent.tool.invoked"
            ]
            tool_outputs = extract_tool_outputs(events)
            print(f"[Agent Continuation {retry+1} Tools Invoked] {called_tools}", flush=True)

        print(f"[Tool Outputs Summary] keys={list(tool_outputs.keys())}", flush=True)

        # Verify Meshy balance after generation
        balance_after = check_meshy_balance(meshy_key)
        print(f"[Meshy] Balance after test: {balance_after}", flush=True)

        # Extract artifacts from tool outputs and validate all 5 Gates!
        gen_out = tool_outputs.get("mcp__gen-model__gen_mesh", {})
        candidates = gen_out.get("candidates", [])
        assert candidates, f"gen_mesh did not produce candidates: {gen_out}"
        raw_ref = candidates[0]["meshFileRef"]
        meshy_meta = candidates[0].get("meta", {})
        raw_abs = resolve_proj_path(raw_ref)
        shutil.copy2(raw_abs, artifacts_dir / "stage2_meshy_raw.glb")

        g2 = validate_gate_2_raw_glb(raw_abs)
        assert g2.passed, f"Gate 2 on raw Meshy GLB failed: {g2.message}"
        print(f"[Gate 2 PASS] ref={raw_ref}, size={raw_abs.stat().st_size} bytes, meshy_meta={meshy_meta}", flush=True)

        refine_out = tool_outputs.get("mcp__gen-model__gen_mesh_refine", {})
        refined_ref = refine_out["refinedMeshFileRef"]
        refined_abs = resolve_proj_path(refined_ref)
        shutil.copy2(refined_abs, artifacts_dir / "stage3_refined.glb")
        g2_ref = validate_gate_2_raw_glb(refined_abs)
        assert g2_ref.passed, f"Gate 2 on refined GLB failed: {g2_ref.message}"
        g3 = validate_gate_3_polycount(refined_abs, target_faces=4000)
        assert g3.passed, f"Gate 3 failed: {g3.message}"
        print(f"[Gate 3 PASS] {refined_ref}: {refine_out.get('originalTriangles')} -> {refine_out.get('refinedTriangles')} tris ({refine_out.get('reductionPercent'):.2f}% reduction)", flush=True)

        rig_out = tool_outputs.get("mcp__gen-model__gen_mesh_rig", {})
        rigged_ref = rig_out["riggedMeshFileRef"]
        rigged_abs = resolve_proj_path(rigged_ref)
        shutil.copy2(rigged_abs, artifacts_dir / "stage4_rigged.glb")
        g4 = validate_gate_4_rigging(rigged_abs)
        assert g4.passed, f"Gate 4 failed: {g4.message}"
        print(f"[Gate 4 PASS] {rigged_ref}: joints={rig_out.get('jointsCount')}, normalized={rig_out.get('normalizedWeights')}", flush=True)

        anim_out = tool_outputs.get("mcp__gen-model__gen_mesh_animate", {})
        anim_ref = anim_out["animatedMeshFileRef"]
        anim_abs = resolve_proj_path(anim_ref)
        shutil.copy2(anim_abs, artifacts_dir / "stage5_animated.glb")

        pub_out = tool_outputs.get("mcp__gen-model__gen_character_publish", {})
        model_guid = pub_out["modelGuid"]
        rxmodel_uri = pub_out["rxmodelUri"]
        rxprefab_uri = pub_out["rxprefabUri"]
        pub_dir = resolve_proj_path(f"Content/Models/{model_guid}")
        shutil.copy2(pub_dir / "model.rxmodel", artifacts_dir / "model.rxmodel")
        shutil.copy2(pub_dir / "template.rxprefab", artifacts_dir / "template.rxprefab")

        g5 = validate_gate_5_motion_publish(anim_abs, epsilon=0.01, published_dir=pub_dir)
        assert g5.passed, f"Gate 5 failed: {g5.message}"
        print(f"[Gate 5 PASS] {rxmodel_uri} + {rxprefab_uri}: maxObservedDelta={g5.details.get('maxObservedDelta')}", flush=True)

        # Copy session events log as proof of Antigravity Gemini 4 execution
        shutil.copy2(events_file, artifacts_dir / f"agent_session_{session_id}.jsonl")

        report = {
            "ok": True,
            "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "durationSec": round(time.time() - start_ts, 2),
            "antigravityChannel": {
                "configured": ag_channel.get("configured"),
                "authMode": ag_channel.get("authMode"),
                "selectedModelId": "antigravity:gemini-4",
                "sessionId": session_id,
                "eventsLogPath": str(artifacts_dir / f"agent_session_{session_id}.jsonl"),
                "invokedTools": called_tools,
            },
            "meshyApi": {
                "balanceBefore": balance_before,
                "balanceAfter": balance_after,
                "taskMeta": meshy_meta,
            },
            "pipelineStages": {
                "stage1_conceptArt": {
                    "path": concept_rel,
                    "gate1Passed": g1.passed,
                    "metrics": g1.details,
                },
                "stage2_genMesh": {
                    "meshFileRef": raw_ref,
                    "byteSize": raw_abs.stat().st_size,
                    "meshyMeta": meshy_meta,
                },
                "stage3_refineMesh": {
                    "refinedMeshFileRef": refined_ref,
                    "originalTriangles": refine_out.get("originalTriangles"),
                    "refinedTriangles": refine_out.get("refinedTriangles"),
                    "originalVertices": refine_out.get("originalVertices"),
                    "refinedVertices": refine_out.get("refinedVertices"),
                    "reductionPercent": refine_out.get("reductionPercent"),
                    "gate3Passed": g3.passed,
                },
                "stage4_rigMesh": {
                    "riggedMeshFileRef": rigged_ref,
                    "jointsCount": rig_out.get("jointsCount"),
                    "normalizedWeights": rig_out.get("normalizedWeights"),
                    "gate4Passed": g4.passed,
                },
                "stage5_animateAndPublish": {
                    "animatedMeshFileRef": anim_ref,
                    "animations": anim_out.get("animations"),
                    "modelGuid": model_guid,
                    "rxmodelUri": rxmodel_uri,
                    "rxprefabUri": rxprefab_uri,
                    "prefabGuid": pub_out.get("prefabGuid"),
                    "maxVertexDeformationDelta": g5.details.get("maxObservedDelta"),
                    "gate5Passed": g5.passed,
                },
            },
        }

        report_path = artifacts_dir / "live_meshy_gemini4_report.json"
        report_path.write_text(
            json.dumps(
                report,
                indent=2,
                ensure_ascii=False,
                default=lambda o: o.item() if hasattr(o, "item") else str(o),
            ),
            encoding="utf-8",
        )
        print(f"\n[LIVE E2E SUCCESS] Report written to: {report_path}", flush=True)
        return report

    finally:
        if orig_forge_toml is not None:
            forge_toml_path.write_text(orig_forge_toml, encoding="utf-8")
        root_models = WORKSPACE_ROOT / "Content" / "Models"
        if root_models.exists():
            dst_models = demo_root / "Content" / "Models"
            dst_models.mkdir(parents=True, exist_ok=True)
            for item in root_models.iterdir():
                target = dst_models / item.name
                if item.is_dir():
                    shutil.copytree(item, target, dirs_exist_ok=True)
                else:
                    shutil.copy2(item, target)
        if (WORKSPACE_ROOT / "Content").exists():
            shutil.rmtree(WORKSPACE_ROOT / "Content", ignore_errors=True)
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except Exception:
            proc.kill()


if __name__ == "__main__":
    run_live_e2e()

