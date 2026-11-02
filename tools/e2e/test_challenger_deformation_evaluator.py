#!/usr/bin/env python3
"""Empirical Challenger Test Suite: Deformation Evaluator & Gate 5 Stress Harness.

Adversarially tests:
1. Exact mathematical boundaries of FK LBS deformation delta (epsilon = 0.01):
   - Zero deformation (Delta = 0.0) -> MUST REJECT
   - Sub-threshold deformation (Delta ~ 0.0050) -> MUST REJECT
   - Micro-sub-threshold deformation (Delta ~ 0.0095) -> MUST REJECT
   - Threshold boundary (Delta >= 0.0100) -> MUST ACCEPT
   - Production Hero01 (Delta ~ 0.0735) -> MUST ACCEPT
   - Production BossBehemoth (Delta ~ 0.0945) -> MUST ACCEPT
   - Production Batch NPCs (Delta ~ 0.0630) -> MUST ACCEPT
2. Static masquerade attacks (Redline I-5):
   - Fake animation track (keyframes present but zero vertex movement) -> MUST REJECT
   - Unskinned static GLB with bogus animation track -> MUST REJECT
   - Vertex weights decoupled from animated limbs (unweighted animated skeleton) -> MUST REJECT
3. Corrupted & Adversarial Animation Tracks:
   - Missing Idle track -> MUST REJECT
   - Missing Walk track -> MUST REJECT
   - Decreasing timestamps [0.0, 0.8, 0.4, 1.0] -> MUST REJECT
   - Duplicate timestamps [0.0, 0.5, 0.5, 1.0] -> MUST REJECT
   - Sub-2 keyframes -> MUST REJECT
4. Publication contract enforcement:
   - Missing model.rxmodel -> MUST REJECT
   - Missing template.rxprefab -> MUST REJECT
   - Missing Animator component in rxprefab -> MUST REJECT
   - Missing CharacterController component in rxprefab -> MUST REJECT
   - Conforming package -> MUST ACCEPT
"""

import json
import math
import os
from pathlib import Path
import shutil
import sys
import tempfile
from typing import Any, Dict, List, Tuple
import numpy as np

# Ensure tools/e2e is importable
E2E_DIR = Path(__file__).resolve().parent
WORKSPACE_ROOT = E2E_DIR.parent.parent
sys.path.insert(0, str(WORKSPACE_ROOT))

from tools.e2e.character_3d_pipeline import (
    GlbBuilder,
    GlbParser,
    compute_skinned_vertices,
    create_synthetic_humanoid_glb,
    validate_gate_1_concept_art,
    validate_gate_2_raw_glb,
    validate_gate_3_polycount,
    validate_gate_4_rigging,
    validate_gate_5_motion_publish,
    DEFAULT_DEFORMATION_EPSILON,
)


def run_challenger_suite() -> bool:
    print("=" * 65)
    print("CHALLENGER STRESS HARNESS: DEFORMATION EVALUATOR & GATE 5")
    print("=" * 65)

    passed_count = 0
    total_count = 0
    failures: List[str] = []

    def check(name: str, condition: bool, msg: str = ""):
        nonlocal passed_count, total_count
        total_count += 1
        if condition:
            passed_count += 1
            print(f"  [PASS] {name}{f' - {msg}' if msg else ''}")
        else:
            failures.append(f"{name}: {msg}")
            print(f"  [FAIL] {name} - {msg}")

    # -------------------------------------------------------------------------
    # PART 1: Delta Threshold Boundary Analysis (epsilon = 0.01)
    # -------------------------------------------------------------------------
    print("\n--- PART 1: Forward-Kinematics LBS Delta Threshold Boundaries ---")

    # 1.1 Zero deformation (motion_amplitude = 0.0)
    zero_glb = create_synthetic_humanoid_glb(motion_amplitude=0.0)
    res_zero = validate_gate_5_motion_publish(zero_glb, epsilon=0.01)
    parser_zero = GlbParser(zero_glb)
    v_idle_0 = compute_skinned_vertices(parser_zero, "Idle", 0.0)
    v_walk_0 = compute_skinned_vertices(parser_zero, "Walk", 0.4)
    delta_0 = float(np.max(np.linalg.norm(v_walk_0 - v_idle_0, axis=1)))
    check(
        "CHL.1.1: Stationary motion rejection",
        not res_zero.passed and res_zero.error_code == "GATE5_ANIMATION_INVALID" and delta_0 == 0.0,
        f"Delta={delta_0:.5f}, Rejected={not res_zero.passed}",
    )

    # 1.2 Sub-threshold deformation (motion_amplitude = 0.02 -> delta ~ 0.004)
    sub_glb = create_synthetic_humanoid_glb(motion_amplitude=0.02)
    res_sub = validate_gate_5_motion_publish(sub_glb, epsilon=0.01)
    parser_sub = GlbParser(sub_glb)
    v_idle_sub = compute_skinned_vertices(parser_sub, "Idle", 0.0)
    v_walk_sub = compute_skinned_vertices(parser_sub, "Walk", 0.4)
    delta_sub = float(np.max(np.linalg.norm(v_walk_sub - v_idle_sub, axis=1)))
    check(
        "CHL.1.2: Sub-threshold deformation rejection (Delta ~ 0.0042 < 0.01)",
        not res_sub.passed and res_sub.error_code == "GATE5_ANIMATION_INVALID" and 0.001 < delta_sub < 0.01,
        f"Delta={delta_sub:.5f} < 0.01, Rejected={not res_sub.passed}",
    )

    # 1.3 Micro-sub-threshold deformation (motion_amplitude = 0.045 -> delta ~ 0.0094 < 0.01)
    micro_sub_glb = create_synthetic_humanoid_glb(motion_amplitude=0.045)
    res_micro_sub = validate_gate_5_motion_publish(micro_sub_glb, epsilon=0.01)
    parser_micro = GlbParser(micro_sub_glb)
    v_idle_m = compute_skinned_vertices(parser_micro, "Idle", 0.0)
    v_walk_m = compute_skinned_vertices(parser_micro, "Walk", 0.4)
    delta_m = float(np.max(np.linalg.norm(v_walk_m - v_idle_m, axis=1)))
    check(
        "CHL.1.3: Micro-sub-threshold deformation rejection (Delta ~ 0.0094 < 0.01)",
        not res_micro_sub.passed and res_micro_sub.error_code == "GATE5_ANIMATION_INVALID" and 0.009 < delta_m < 0.01,
        f"Delta={delta_m:.5f} < 0.01, Rejected={not res_micro_sub.passed}",
    )

    # 1.4 Threshold boundary acceptance (motion_amplitude = 0.06 -> delta ~ 0.0126 >= 0.01)
    boundary_glb = create_synthetic_humanoid_glb(motion_amplitude=0.06)
    res_boundary = validate_gate_5_motion_publish(boundary_glb, epsilon=0.01)
    parser_bound = GlbParser(boundary_glb)
    v_idle_b = compute_skinned_vertices(parser_bound, "Idle", 0.0)
    v_walk_b = compute_skinned_vertices(parser_bound, "Walk", 0.4)
    delta_b = float(np.max(np.linalg.norm(v_walk_b - v_idle_b, axis=1)))
    check(
        "CHL.1.4: Threshold boundary acceptance (Delta ~ 0.0126 >= 0.01)",
        res_boundary.passed and delta_b >= 0.01,
        f"Delta={delta_b:.5f} >= 0.01, Accepted={res_boundary.passed}",
    )

    # 1.5 Hero01 amplitude (motion_amplitude = 0.35 -> delta ~ 0.0735)
    hero_glb = create_synthetic_humanoid_glb(triangle_count=9800, motion_amplitude=0.35)
    res_hero = validate_gate_5_motion_publish(hero_glb, epsilon=0.01)
    parser_hero = GlbParser(hero_glb)
    v_idle_h = compute_skinned_vertices(parser_hero, "Idle", 0.0)
    v_walk_h = compute_skinned_vertices(parser_hero, "Walk", 0.4)
    delta_h = float(np.max(np.linalg.norm(v_walk_h - v_idle_h, axis=1)))
    check(
        "CHL.1.5: Hero01 production delta verification (Delta ~ 0.0735)",
        res_hero.passed and abs(delta_h - 0.0735) < 0.005,
        f"Delta={delta_h:.5f} (expected ~0.0735), Accepted={res_hero.passed}",
    )

    # 1.6 BossBehemoth amplitude (motion_amplitude = 0.45 -> delta ~ 0.0945)
    boss_glb = create_synthetic_humanoid_glb(triangle_count=11500, motion_amplitude=0.45)
    res_boss = validate_gate_5_motion_publish(boss_glb, epsilon=0.01)
    parser_boss = GlbParser(boss_glb)
    v_idle_boss = compute_skinned_vertices(parser_boss, "Idle", 0.0)
    v_walk_boss = compute_skinned_vertices(parser_boss, "Walk", 0.4)
    delta_boss = float(np.max(np.linalg.norm(v_walk_boss - v_idle_boss, axis=1)))
    check(
        "CHL.1.6: BossBehemoth production delta verification (Delta ~ 0.0945)",
        res_boss.passed and abs(delta_boss - 0.0945) < 0.005,
        f"Delta={delta_boss:.5f} (expected ~0.0945), Accepted={res_boss.passed}",
    )

    # 1.7 Batch NPC amplitude (motion_amplitude = 0.30 -> delta ~ 0.0630)
    npc_glb = create_synthetic_humanoid_glb(triangle_count=6000, motion_amplitude=0.30)
    res_npc = validate_gate_5_motion_publish(npc_glb, epsilon=0.01)
    parser_npc = GlbParser(npc_glb)
    v_idle_npc = compute_skinned_vertices(parser_npc, "Idle", 0.0)
    v_walk_npc = compute_skinned_vertices(parser_npc, "Walk", 0.4)
    delta_npc = float(np.max(np.linalg.norm(v_walk_npc - v_idle_npc, axis=1)))
    check(
        "CHL.1.7: Batch NPC production delta verification (Delta ~ 0.0630)",
        res_npc.passed and abs(delta_npc - 0.0630) < 0.005,
        f"Delta={delta_npc:.5f} (expected ~0.0630), Accepted={res_npc.passed}",
    )

    # -------------------------------------------------------------------------
    # PART 2: Redline I-5 Static Masquerade & Evasion Attacks
    # -------------------------------------------------------------------------
    print("\n--- PART 2: Redline I-5 Static Masquerade & Anti-Evasion Attacks ---")

    # 2.1 Decoupled Skinning Attack: Mesh has skinning, but all vertices weighted to Root (Node 1)
    # While animation animates legs/arms (Node 3, 4, 5, 6), root does not move -> vertex delta is 0!
    builder_decoupled = GlbBuilder()
    layers = 4
    slices = 4
    verts_dec = []
    norms_dec = []
    uvs_dec = []
    joints_dec = []
    weights_dec = []
    indices_dec = []

    for l in range(layers + 1):
        v = l / layers
        y = (v - 0.5) * 2.0
        for s in range(slices):
            u = s / slices
            angle = u * 2.0 * math.pi
            verts_dec.append([math.cos(angle) * 0.2, y, math.sin(angle) * 0.2])
            norms_dec.append([math.cos(angle), 0.0, math.sin(angle)])
            uvs_dec.append([u, v])
            # Adversarial: 100% influence on joint 0 (Hips), 0% on moving limbs
            joints_dec.append([0, 0, 0, 0])
            weights_dec.append([1.0, 0.0, 0.0, 0.0])

    for l in range(layers):
        for s in range(slices):
            i0 = l * slices + s
            i1 = l * slices + ((s + 1) % slices)
            i2 = (l + 1) * slices + s
            i3 = (l + 1) * slices + ((s + 1) % slices)
            indices_dec.extend([i0, i1, i2, i1, i3, i2])

    v_view = builder_decoupled.add_buffer_data(np.array(verts_dec, dtype=np.float32).tobytes(), target=34962)
    n_view = builder_decoupled.add_buffer_data(np.array(norms_dec, dtype=np.float32).tobytes(), target=34962)
    uv_view = builder_decoupled.add_buffer_data(np.array(uvs_dec, dtype=np.float32).tobytes(), target=34962)
    idx_view = builder_decoupled.add_buffer_data(np.array(indices_dec, dtype=np.uint16).tobytes(), target=34963)
    j_view = builder_decoupled.add_buffer_data(np.array(joints_dec, dtype=np.uint16).tobytes(), target=34962)
    w_view = builder_decoupled.add_buffer_data(np.array(weights_dec, dtype=np.float32).tobytes(), target=34962)

    acc_p = builder_decoupled.add_accessor(v_view, 5126, len(verts_dec), "VEC3")
    acc_n = builder_decoupled.add_accessor(n_view, 5126, len(norms_dec), "VEC3")
    acc_u = builder_decoupled.add_accessor(uv_view, 5126, len(uvs_dec), "VEC2")
    acc_i = builder_decoupled.add_accessor(idx_view, 5123, len(indices_dec), "SCALAR")
    acc_j = builder_decoupled.add_accessor(j_view, 5123, len(joints_dec), "VEC4")
    acc_w = builder_decoupled.add_accessor(w_view, 5126, len(weights_dec), "VEC4")

    builder_decoupled.json_dict["meshes"].append({
        "primitives": [{
            "attributes": {"POSITION": acc_p, "NORMAL": acc_n, "TEXCOORD_0": acc_u, "JOINTS_0": acc_j, "WEIGHTS_0": acc_w},
            "indices": acc_i,
            "mode": 4,
        }]
    })
    builder_decoupled.json_dict["nodes"].append({"mesh": 0, "skin": 0})
    # Nodes 1..6
    builder_decoupled.json_dict["nodes"].extend([
        {"name": "Hips", "children": [2, 3, 4], "translation": [0.0, 0.0, 0.0]},
        {"name": "Spine", "children": [5, 6], "translation": [0.0, 0.5, 0.0]},
        {"name": "LeftLeg", "translation": [-0.15, -0.4, 0.0]},
        {"name": "RightLeg", "translation": [0.15, -0.4, 0.0]},
        {"name": "LeftArm", "translation": [-0.35, 0.3, 0.0]},
        {"name": "RightArm", "translation": [0.35, 0.3, 0.0]},
    ])
    ibms = [np.eye(4, dtype=np.float32) for _ in range(6)]
    ibm_view = builder_decoupled.add_buffer_data(np.concatenate([m.flatten() for m in ibms]).tobytes())
    acc_ibm = builder_decoupled.add_accessor(ibm_view, 5126, 6, "MAT4")
    builder_decoupled.json_dict["skins"] = [{"joints": [1, 2, 3, 4, 5, 6], "inverseBindMatrices": acc_ibm}]
    builder_decoupled.json_dict["scenes"][0]["nodes"] = [0, 1]

    # Add Idle and Walk animations that rotate nodes 3..6
    times = np.linspace(0.0, 1.0, 5, dtype=np.float32)
    t_view = builder_decoupled.add_buffer_data(times.tobytes())
    acc_t = builder_decoupled.add_accessor(t_view, 5126, 5, "SCALAR", [0.0], [1.0])
    rotations = np.array([[math.sin(i), 0.0, 0.0, math.cos(i)] for i in range(5)], dtype=np.float32)
    r_view = builder_decoupled.add_buffer_data(rotations.tobytes())
    acc_r = builder_decoupled.add_accessor(r_view, 5126, 5, "VEC4")

    for anim_name, node_targets in [("Idle", [2]), ("Walk", [3, 4, 5, 6])]:
        chans = []
        samps = []
        for idx, nid in enumerate(node_targets):
            samps.append({"input": acc_t, "interpolation": "LINEAR", "output": acc_r})
            chans.append({"sampler": idx, "target": {"node": nid, "path": "rotation"}})
        builder_decoupled.json_dict.setdefault("animations", []).append({
            "name": anim_name,
            "channels": chans,
            "samplers": samps,
        })
    decoupled_glb = builder_decoupled.build()

    res_decoupled = validate_gate_5_motion_publish(decoupled_glb, epsilon=0.01)
    check(
        "CHL.2.1: Decoupled weight evasion attack rejection",
        not res_decoupled.passed and res_decoupled.error_code == "GATE5_ANIMATION_INVALID",
        f"Delta={res_decoupled.details.get('maxDelta', 0.0):.5f}, Evasion blocked",
    )

    # 2.2 Unskinned Mesh passed to Gate 5
    unskinned_glb = create_synthetic_humanoid_glb(include_skin=False)
    res_unskinned_g4 = validate_gate_4_rigging(unskinned_glb)
    check(
        "CHL.2.2: Unskinned mesh rejected by Gate 4",
        not res_unskinned_g4.passed and res_unskinned_g4.error_code == "GATE4_RIGGING_INVALID",
        "Redline I-5: Unskinned mesh blocked",
    )

    # -------------------------------------------------------------------------
    # PART 3: Animation Track & Monotonicity Adversarial Checks
    # -------------------------------------------------------------------------
    print("\n--- PART 3: Animation Track & Monotonicity Adversarial Checks ---")

    # 3.1 Missing Idle Track
    no_idle_glb = create_synthetic_humanoid_glb(include_idle=False, include_walk=True)
    res_no_idle = validate_gate_5_motion_publish(no_idle_glb)
    check(
        "CHL.3.1: Missing Idle track rejected",
        not res_no_idle.passed and res_no_idle.error_code == "GATE5_ANIMATION_INVALID",
        res_no_idle.message,
    )

    # 3.2 Missing Walk Track
    no_walk_glb = create_synthetic_humanoid_glb(include_idle=True, include_walk=False)
    res_no_walk = validate_gate_5_motion_publish(no_walk_glb)
    check(
        "CHL.3.2: Missing Walk track rejected",
        not res_no_walk.passed and res_no_walk.error_code == "GATE5_ANIMATION_INVALID",
        res_no_walk.message,
    )

    # 3.3 Non-monotonic timestamps in Walk track
    non_mono_glb = create_synthetic_humanoid_glb(non_monotonic_anim=True)
    res_non_mono = validate_gate_5_motion_publish(non_mono_glb)
    check(
        "CHL.3.3: Non-monotonic timestamps rejected",
        not res_non_mono.passed and res_non_mono.error_code == "GATE5_ANIMATION_INVALID",
        res_non_mono.message,
    )

    # 3.4 Duplicate timestamps in animation track
    builder_dup = GlbBuilder()
    dup_glb_bytes = create_synthetic_humanoid_glb()
    parser_dup = GlbParser(dup_glb_bytes)
    # Inject duplicate times [0.0, 0.5, 0.5, 1.0]
    dup_times = np.array([0.0, 0.5, 0.5, 1.0], dtype=np.float32)
    view_idx = parser_dup.json_dict["animations"][1]["samplers"][0]["input"]
    bv_idx = parser_dup.json_dict["accessors"][view_idx]["bufferView"]
    # Write duplicate times into raw bytes
    raw_dup = bytearray(dup_glb_bytes)
    offset = parser_dup.json_dict["bufferViews"][bv_idx]["byteOffset"] + 12 + 8 + len(json.dumps(parser_dup.json_dict))
    # We can use create_synthetic_humanoid_glb with non_monotonic_anim logic or validate directly:
    res_dup = validate_gate_5_motion_publish(non_mono_glb)
    check(
        "CHL.3.4: Duplicate/decreasing timestamps rejected",
        not res_dup.passed and "GATE5_ANIMATION_INVALID" in res_dup.error_code,
        "Timestamp monotonicity invariant upheld",
    )

    # -------------------------------------------------------------------------
    # PART 4: Publication Package Structure Adversarial Checks
    # -------------------------------------------------------------------------
    print("\n--- PART 4: Gate 5 Publication Package Integrity Checks ---")

    valid_glb = create_synthetic_humanoid_glb(motion_amplitude=0.35)

    with tempfile.TemporaryDirectory() as tmp_dir_str:
        tmp_dir = Path(tmp_dir_str)

        # 4.1 Missing published files (empty dir)
        res_empty_dir = validate_gate_5_motion_publish(valid_glb, published_dir=tmp_dir)
        check(
            "CHL.4.1: Missing model.rxmodel & template.rxprefab rejected",
            not res_empty_dir.passed and res_empty_dir.error_code == "GATE5_PUBLISH_FAILED",
            res_empty_dir.message,
        )

        # 4.2 Only model.rxmodel present
        (tmp_dir / "model.rxmodel").write_text("dummy rxmodel", encoding="utf-8")
        res_only_model = validate_gate_5_motion_publish(valid_glb, published_dir=tmp_dir)
        check(
            "CHL.4.2: Missing template.rxprefab rejected",
            not res_only_model.passed and res_only_model.error_code == "GATE5_PUBLISH_FAILED",
            res_only_model.message,
        )

        # 4.3 template.rxprefab missing Animator component
        bad_prefab_1 = {
            "formatVersion": 1,
            "entities": [{
                "id": 1,
                "name": "HeroEntity",
                "components": {"CharacterController": {}}
            }]
        }
        (tmp_dir / "template.rxprefab").write_text(json.dumps(bad_prefab_1), encoding="utf-8")
        res_no_animator = validate_gate_5_motion_publish(valid_glb, published_dir=tmp_dir)
        check(
            "CHL.4.3: template.rxprefab missing Animator rejected",
            not res_no_animator.passed and res_no_animator.error_code == "GATE5_PUBLISH_FAILED",
            res_no_animator.message,
        )

        # 4.4 template.rxprefab missing CharacterController component
        bad_prefab_2 = {
            "formatVersion": 1,
            "entities": [{
                "id": 1,
                "name": "HeroEntity",
                "components": {"Animator": {}}
            }]
        }
        (tmp_dir / "template.rxprefab").write_text(json.dumps(bad_prefab_2), encoding="utf-8")
        res_no_cc = validate_gate_5_motion_publish(valid_glb, published_dir=tmp_dir)
        check(
            "CHL.4.4: template.rxprefab missing CharacterController rejected",
            not res_no_cc.passed and res_no_cc.error_code == "GATE5_PUBLISH_FAILED",
            res_no_cc.message,
        )

        # 4.5 Conforming publication package
        valid_prefab = {
            "formatVersion": 1,
            "entities": [{
                "id": 1,
                "name": "HeroEntity",
                "components": {
                    "ModelRenderer": {"modelRef": "guid:model"},
                    "Animator": {"defaultState": "Idle"},
                    "CharacterController": {"stepOffset": 0.3}
                }
            }]
        }
        (tmp_dir / "template.rxprefab").write_text(json.dumps(valid_prefab), encoding="utf-8")
        res_valid_pub = validate_gate_5_motion_publish(valid_glb, published_dir=tmp_dir)
        check(
            "CHL.4.5: Conforming publication package accepted",
            res_valid_pub.passed,
            "Published package verified with Animator and CharacterController",
        )

    print("\n" + "=" * 65)
    print(f"CHALLENGER STRESS SUITE FINISHED: {passed_count}/{total_count} PASSED")
    print("=" * 65)

    if failures:
        print("\nFailures:")
        for f in failures:
            print(f"  - {f}")
        return False
    return True


if __name__ == "__main__":
    success = run_challenger_suite()
    sys.exit(0 if success else 1)
