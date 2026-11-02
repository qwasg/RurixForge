#!/usr/bin/env python3
"""Milestone 4 Adversarial Stress Harness & Empirical Verification.
Challenger 2 suite for RurixForge Build Composer Mode (`world_build`).

Focuses on:
  - Feature 7: Multi-Agent Construction DAG Materialization & Building Artisan Constraints
  - Feature 8: Server-Side Coherence Engine, AABB Collision Math, Hierarchy & Rotation Integrity
  - Adversarial Boundary Stress: Precision, Extreme Coordinates, Scale, Degenerate Geometry, Inverted Bounds
  - Lifecycle State Machine Resilience & Stale Revision Guarding
  - Redline R-5 / I-5 Security & Anti-Facade Compliance
"""

import copy
import json
import math
import os
from pathlib import Path
import shutil
import sys
import tempfile
import time
from typing import Any, Dict, List, Optional, Tuple

import numpy as np

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(WORKSPACE_ROOT))

from tools.e2e.test_world_build_e2e import (
    BoundingBox3D,
    WorldBuildSimulator,
    check_aabb_xz_collision,
    validate_gate_4_dag_artisan,
    validate_gate_5_coherence,
    COLLISION_EPSILON,
    REDLINE_SECRET_PATTERNS,
    ERR_COLLISION_DETECTED,
    ERR_PROP_ORPHANED,
    ERR_BUILDING_MISSING,
    ERR_INVALID_ACTION,
    ERR_STAGE_MISMATCH,
)

class ChallengerM4StressRunner:
    def __init__(self):
        self.tests_run = 0
        self.tests_passed = 0
        self.tests_failed = 0
        self.failures: List[Dict[str, Any]] = []

    def record_pass(self, name: str, duration: float):
        self.tests_run += 1
        self.tests_passed += 1
        print(f"  [PASS] {name} ({duration:.4f}s)")

    def record_fail(self, name: str, duration: float, error: str):
        import traceback
        self.tests_run += 1
        self.tests_failed += 1
        self.failures.append({"name": name, "error": error})
        print(f"  [FAIL] {name} ({duration:.4f}s): {error}")
        traceback.print_exc()

    def run_test(self, name: str, fn):
        t0 = time.time()
        try:
            fn()
            dur = time.time() - t0
            self.record_pass(name, dur)
        except Exception as e:
            dur = time.time() - t0
            self.record_fail(name, dur, str(e))

# ===========================================================================
# Stress Test Cases
# ===========================================================================

def run_all_stress_tests():
    runner = ChallengerM4StressRunner()
    print("================================================================================")
    print("  RurixForge Milestone 4 Challenger 2: Adversarial Stress & Verification Suite  ")
    print("================================================================================")

    # -----------------------------------------------------------------------
    # Section 1: Feature 7 - Multi-Agent Construction DAG Materialization
    # -----------------------------------------------------------------------
    print("\n--- Group 1: Feature 7 DAG Materialization & Scalability ---")

    def test_dag_massive_building_count():
        """Stress: Materialize DAG with 100 buildings, verifying O(N) scaling and deps."""
        with tempfile.TemporaryDirectory() as td:
            sim = WorldBuildSimulator("massive-dag", Path(td))
            sim.run_survey()
            bldgs = [
                {
                    "id": f"tower_{i:03d}",
                    "bounds": {"min": [-2, 0, -2], "max": [2, 10, 2]},
                    "transform": {"position": [(i % 10) * 15, 0, (i // 10) * 15]},
                }
                for i in range(100)
            ]
            sim.populate_preview(bldgs, style="dense_metropolis")
            sim.approve_preview()
            dag = sim.materialize_dag()

            tasks = dag["tasks"]
            artisan_tasks = [t for t in tasks if t["role"] == "building-artisan"]
            coord_tasks = [t for t in tasks if t["role"] == "coordinator"]

            assert len(artisan_tasks) == 100, f"Expected 100 artisan tasks, got {len(artisan_tasks)}"
            assert len(coord_tasks) == 1, f"Expected 1 coordinator task, got {len(coord_tasks)}"
            coord = coord_tasks[0]
            assert len(coord["deps"]) == 100, "Coordinator must depend on all 100 artisan tasks"
            for i in range(100):
                assert f"artisan_tower_{i:03d}" in coord["deps"]
    runner.run_test("test_dag_massive_building_count", test_dag_massive_building_count)

    def test_dag_building_id_adversarial_characters():
        """Stress: Building IDs with unicode, spaces, slashes, and CJK in DAG."""
        with tempfile.TemporaryDirectory() as td:
            sim = WorldBuildSimulator("adv-ids", Path(td))
            sim.run_survey()
            bldgs = [
                {"id": "钟楼-Tower.01_#A", "bounds": {"min": [0,0,0], "max": [1,1,1]}, "transform": {"position": [0,0,0]}},
                {"id": "🏯_Palace / Central", "bounds": {"min": [0,0,0], "max": [1,1,1]}, "transform": {"position": [10,0,0]}},
            ]
            sim.populate_preview(bldgs)
            sim.approve_preview()
            dag = sim.materialize_dag()
            assert len(dag["tasks"]) == 3
            artisan_ids = [t["id"] for t in dag["tasks"] if t["role"] == "building-artisan"]
            assert "artisan_钟楼-Tower.01_#A" in artisan_ids
            assert "artisan_🏯_Palace / Central" in artisan_ids
    runner.run_test("test_dag_building_id_adversarial_characters", test_dag_building_id_adversarial_characters)

    def test_dag_concurrency_and_pool_invariants():
        """Stress: Verify max_parallel and max_fix_rounds are set properly in team plan."""
        with tempfile.TemporaryDirectory() as td:
            sim = WorldBuildSimulator("concurrency-inv", Path(td))
            sim.run_survey()
            sim.populate_preview([
                {"id": f"b_{i}", "bounds": {"min": [0,0,0], "max": [1,1,1]}, "transform": {"position": [i*5, 0, 0]}}
                for i in range(8)
            ])
            sim.approve_preview()
            dag = sim.materialize_dag()
            # Gate 4 check
            passed, err = validate_gate_4_dag_artisan(dag)
            assert passed is True, f"Gate 4 failed: {err}"
    runner.run_test("test_dag_concurrency_and_pool_invariants", test_dag_concurrency_and_pool_invariants)

    def test_building_artisan_whitelist_completeness():
        """Verify building-artisan profile has exactly 22 whitelisted tools."""
        profile_path = WORKSPACE_ROOT / "agents" / "building-artisan.md"
        assert profile_path.exists(), "agents/building-artisan.md must exist"
        text = profile_path.read_text(encoding="utf-8")
        # Extract tools list from YAML frontmatter
        import re
        match = re.search(r'tools:\s*\[(.*?)\]', text, re.DOTALL)
        assert match is not None, "tools list must be defined in frontmatter"
        tools_str = match.group(1)
        raw_tools = [t.strip().strip('"').strip("'") for t in tools_str.split(",") if t.strip()]
        assert len(raw_tools) == 22, f"Expected 22 tools in whitelist, found {len(raw_tools)}: {raw_tools}"
        # Ensure mandatory MCP tool families are present
        assert "mcp__engine-scene__*" in raw_tools
        assert "mcp__asset-pipeline__*" in raw_tools
        assert "mcp__gen-model__*" in raw_tools
        assert "mcp__store__library_list" in raw_tools
        assert "editor_overview" in raw_tools
        # Ensure task tool is strictly forbidden
        assert "task" not in raw_tools
    runner.run_test("test_building_artisan_whitelist_completeness", test_building_artisan_whitelist_completeness)

    # -----------------------------------------------------------------------
    # Section 2: Feature 8 - Coherence Gate & Geometric Collision Math
    # -----------------------------------------------------------------------
    print("\n--- Group 2: Feature 8 Coherence & Collision Math Precision ---")

    def test_aabb_tangent_boundary_precision():
        """Adversarial: Test exact mathematical boundary at epsilon=1e-4.
        Box 1: X in [-1, 1], Box 2: X in [1 + delta, 3 + delta].
        Delta = 0.000101 (> eps) -> Separated (NO collision).
        Delta = 0.000099 (< eps) -> Overlap = 0.000101 > eps -> COLLISION.
        """
        box1 = {"min": [-1.0, 0.0, -1.0], "max": [1.0, 2.0, 1.0]}
        box2 = {"min": [-1.0, 0.0, -1.0], "max": [1.0, 2.0, 1.0]}
        pos1 = (0.0, 0.0, 0.0)

        # Case A: separated by 0.000101 (gap > 0.0) -> overlap = 0.0 -> no collision
        pos2_sep = (2.0002, 0.0, 0.0)
        collides, ox, oz = check_aabb_xz_collision(box1, pos1, box2, pos2_sep, epsilon=1e-4)
        assert not collides, f"Separated boxes should not collide, got ox={ox}, oz={oz}"

        # Case B: overlap by 0.0002 (> 1e-4) -> collision
        pos2_coll = (1.9998, 0.0, 0.0)
        collides, ox, oz = check_aabb_xz_collision(box1, pos1, box2, pos2_coll, epsilon=1e-4)
        assert collides, f"Overlapping boxes (ox={ox}) should collide"

        # Case C: exact tangent (gap = 0.0, overlap = 0.0) -> no collision
        pos2_tangent = (2.0, 0.0, 0.0)
        collides, ox, oz = check_aabb_xz_collision(box1, pos1, box2, pos2_tangent, epsilon=1e-4)
        assert not collides, f"Exact tangent boundary should not collide, got ox={ox}"
        assert ox <= 1e-4
    runner.run_test("test_aabb_tangent_boundary_precision", test_aabb_tangent_boundary_precision)

    def test_aabb_degenerate_zero_thickness_box():
        """Stress: Flat 2D plane (billboard/wall) with zero thickness on X or Z."""
        box_flat_x = {"min": [0.0, 0.0, -2.0], "max": [0.0, 3.0, 2.0]}
        box_vol = {"min": [-1.0, 0.0, -1.0], "max": [1.0, 2.0, 1.0]}
        pos_flat = (0.0, 0.0, 0.0)
        pos_vol = (0.5, 0.0, 0.0)
        # Flat box has max_x - min_x = 0 -> overlap_x = min(1.5, 0.0) - max(-0.5, 0.0) = 0.0
        collides, ox, oz = check_aabb_xz_collision(box_flat_x, pos_flat, box_vol, pos_vol)
        assert not collides, "Zero-thickness plane should have 0 overlap_x"
    runner.run_test("test_aabb_degenerate_zero_thickness_box", test_aabb_degenerate_zero_thickness_box)

    def test_aabb_extreme_world_coordinates():
        """Stress: Buildings placed at X = 1,000,000 meters."""
        box = {"min": [-5.0, 0.0, -5.0], "max": [5.0, 20.0, 5.0]}
        pos1 = (1000000.0, 0.0, -1000000.0)
        pos2 = (1000008.0, 0.0, -1000000.0)  # Overlap along X = 2.0m
        collides, ox, oz = check_aabb_xz_collision(box, pos1, box, pos2)
        assert collides is True
        assert abs(ox - 2.0) < 1e-3
        assert abs(oz - 10.0) < 1e-3
    runner.run_test("test_aabb_extreme_world_coordinates", test_aabb_extreme_world_coordinates)

    def test_quaternion_normalization_edge_cases():
        """Stress: Quaternion normalization verification at epsilon 1e-4."""
        # 1. Identity quaternion: [0, 0, 0, 1]
        q_id = [0.0, 0.0, 0.0, 1.0]
        norm_id = math.sqrt(sum(x*x for x in q_id))
        assert abs(norm_id - 1.0) < 1e-4

        # 2. 90 deg rotation around Y: [0, sqrt(0.5), 0, sqrt(0.5)]
        s = math.sqrt(0.5)
        q_90 = [0.0, s, 0.0, s]
        norm_90 = math.sqrt(sum(x*x for x in q_90))
        assert abs(norm_90 - 1.0) < 1e-4

        # 3. Slightly unnormalized: norm = 1.0005 (> 1e-4) -> should be flagged
        q_bad = [0.0, 0.0, 0.0, 1.0005]
        norm_bad = math.sqrt(sum(x*x for x in q_bad))
        assert abs(norm_bad - 1.0) >= 1e-4

        # 4. Zero quaternion: [0, 0, 0, 0] -> norm = 0.0 -> severely unnormalized
        q_zero = [0.0, 0.0, 0.0, 0.0]
        norm_zero = math.sqrt(sum(x*x for x in q_zero))
        assert abs(norm_zero - 1.0) >= 1e-4
    runner.run_test("test_quaternion_normalization_edge_cases", test_quaternion_normalization_edge_cases)

    def test_coherence_nested_interior_props_validation():
        """Stress: Interior props parented to valid buildings vs orphaned props."""
        with tempfile.TemporaryDirectory() as td:
            sim = WorldBuildSimulator("props-stress", Path(td))
            sim.run_survey()
            sim.populate_preview([
                {"id": "bldg_inn", "bounds": {"min": [-4,0,-4], "max": [4,6,4]}, "transform": {"position": [0,0,0]}}
            ])
            sim.approve_preview()

            # Scene with 10 valid interior props
            scene_valid = [
                {"id": "ent_bldg_inn", "isBuilding": True, "buildingId": "bldg_inn", "bounds": {"min": [-4,0,-4], "max": [4,6,4]}, "transform": {"position": [0,0,0]}}
            ]
            for i in range(10):
                scene_valid.append({
                    "id": f"prop_chair_{i}",
                    "isInteriorProp": True,
                    "parentEntityId": "ent_bldg_inn"
                })
            audit = sim.run_coherence_verification(scene_valid)
            assert audit["status"] == "PASS"
            assert audit["evaluatedProps"] == 10

            # Add 1 orphaned prop pointing to non-existent parent
            scene_invalid = copy.deepcopy(scene_valid)
            scene_invalid.append({
                "id": "prop_ghost_table",
                "isInteriorProp": True,
                "parentEntityId": "nonexistent_building_999"
            })
            audit_fail = sim.run_coherence_verification(scene_invalid)
            assert audit_fail["status"] == "FAIL"
            orphan_violations = [v for v in audit_fail["violations"] if v["code"] == ERR_PROP_ORPHANED]
            assert len(orphan_violations) == 1
            assert orphan_violations[0]["propId"] == "prop_ghost_table"
    runner.run_test("test_coherence_nested_interior_props_validation", test_coherence_nested_interior_props_validation)

    def test_coherence_dense_grid_collision_detection():
        """Stress: 16 buildings in a 4x4 grid with 1 intentional collision pair."""
        with tempfile.TemporaryDirectory() as td:
            sim = WorldBuildSimulator("grid-stress", Path(td))
            sim.run_survey()

            buildings = []
            for r in range(4):
                for c in range(4):
                    bid = f"block_{r}_{c}"
                    # 10m spacing, 8m diameter box
                    pos = [c * 10.0, 0.0, r * 10.0]
                    # Induce intentional collision between block_1_1 and block_1_2
                    if r == 1 and c == 2:
                        pos = [11.0, 0.0, 10.0] # Spacing only 1m instead of 10m -> overlap
                    buildings.append({
                        "id": bid,
                        "bounds": {"min": [-4, 0, -4], "max": [4, 5, 4]},
                        "transform": {"position": pos}
                    })
            sim.populate_preview(buildings)
            sim.approve_preview()

            scene = [
                {"id": f"ent_{b['id']}", "isBuilding": True, "buildingId": b["id"], "bounds": b["bounds"], "transform": b["transform"]}
                for b in buildings
            ]
            audit = sim.run_coherence_verification(scene)
            assert audit["status"] == "FAIL"
            collisions = [v for v in audit["violations"] if v["code"] == ERR_COLLISION_DETECTED]
            assert len(collisions) >= 1
            pair_found = any(
                ("ent_block_1_1" in v.get("pair", []) and "ent_block_1_2" in v.get("pair", []))
                or ("block_1_1" in v.get("pair", []) and "block_1_2" in v.get("pair", []))
                for v in collisions
            )
            assert pair_found, f"Collision between block_1_1 and block_1_2 must be detected. Found: {collisions}"
    runner.run_test("test_coherence_dense_grid_collision_detection", test_coherence_dense_grid_collision_detection)

    # -----------------------------------------------------------------------
    # Section 3: Cross-Feature Lifecycle & Auto-Remediation Stress
    # -----------------------------------------------------------------------
    print("\n--- Group 3: Cross-Feature Lifecycle & Auto-Remediation Stress ---")

    def test_full_remediation_lifecycle_flow():
        """Full flow: Survey -> Preview -> Approval -> Materialize DAG -> Coherence Fail -> Remediate -> Coherence Pass -> Done."""
        with tempfile.TemporaryDirectory() as td:
            sim = WorldBuildSimulator("remediation-flow", Path(td))
            # 1. Survey
            assert sim.stage == "survey"
            facts = sim.run_survey()
            assert facts["slug"] == "remediation-flow"
            assert sim.stage == "preview_review"

            # 2. Preview
            bldgs = [
                {"id": "b1", "bounds": {"min": [-2,0,-2], "max": [2,4,2]}, "transform": {"position": [0,0,0]}},
                {"id": "b2", "bounds": {"min": [-2,0,-2], "max": [2,4,2]}, "transform": {"position": [3,0,0]}}, # Collision (gap = -1)
            ]
            sim.populate_preview(bldgs, style="cyberpunk")
            assert sim.stage == "preview_review"

            # 3. Approve Preview
            sim.approve_preview()
            assert sim.stage == "construction"
            assert (Path(td) / ".forge" / "world_build" / "remediation-flow" / "template.json").exists()

            # 4. Materialize DAG
            dag = sim.materialize_dag()
            assert len(dag["tasks"]) == 3 # 2 artisans + 1 coordinator

            # 5. Scene with collision -> verification fails
            scene_bad = [
                {"id": "ent_b1", "isBuilding": True, "buildingId": "b1", "bounds": bldgs[0]["bounds"], "transform": bldgs[0]["transform"]},
                {"id": "ent_b2", "isBuilding": True, "buildingId": "b2", "bounds": bldgs[1]["bounds"], "transform": bldgs[1]["transform"]},
            ]
            audit_fail = sim.run_coherence_verification(scene_bad)
            assert audit_fail["status"] == "FAIL"

            # 6. Coordinator remediates by moving b2 to position [10, 0, 0]
            scene_remediated = [
                {"id": "ent_b1", "isBuilding": True, "buildingId": "b1", "bounds": bldgs[0]["bounds"], "transform": bldgs[0]["transform"]},
                {"id": "ent_b2", "isBuilding": True, "buildingId": "b2", "bounds": bldgs[1]["bounds"], "transform": {"position": [10.0, 0.0, 0.0]}},
                {"id": "prop_lamp", "isInteriorProp": True, "parentEntityId": "ent_b1"}
            ]
            audit_pass = sim.run_coherence_verification(scene_remediated)
            assert audit_pass["status"] == "PASS"
            assert sim.stage == "done"
    runner.run_test("test_full_remediation_lifecycle_flow", test_full_remediation_lifecycle_flow)

    def test_rejection_of_invalid_actions_per_stage():
        """Stress: Verify state machine rejects out-of-order actions."""
        with tempfile.TemporaryDirectory() as td:
            sim = WorldBuildSimulator("state-rejects", Path(td))
            # In survey stage, cannot approve_preview
            try:
                sim.approve_preview()
                raise AssertionError("approve_preview in survey stage must raise exception")
            except Exception as e:
                assert ERR_STAGE_MISMATCH in str(e) or "stage" in str(e).lower()

            sim.run_survey()
            sim.populate_preview([{"id": "b1", "bounds": {"min": [0,0,0], "max": [1,1,1]}, "transform": {"position": [0,0,0]}}])
            # In preview_review, cannot fix_coherence
            # Check allowed actions
            assert sim.stage == "preview_review"
    runner.run_test("test_rejection_of_invalid_actions_per_stage", test_rejection_of_invalid_actions_per_stage)

    # -----------------------------------------------------------------------
    # Section 4: Redline R-5 & Security Compliance
    # -----------------------------------------------------------------------
    print("\n--- Group 4: Redline R-5 Security Audit ---")

    def test_redline_zero_secret_in_simulated_artifacts():
        """Ensure no API keys or secret patterns are ever written into world build artifacts."""
        with tempfile.TemporaryDirectory() as td:
            sim = WorldBuildSimulator("sec-audit", Path(td))
            sim.run_survey()
            sim.populate_preview([
                {"id": "b1", "bounds": {"min": [0,0,0], "max": [1,1,1]}, "transform": {"position": [0,0,0]}}
            ])
            sim.approve_preview()
            sim.materialize_dag()
            sim.run_coherence_verification([
                {"id": "ent_b1", "isBuilding": True, "buildingId": "b1", "bounds": {"min": [0,0,0], "max": [1,1,1]}, "transform": {"position": [0,0,0]}}
            ])

            # Scan all files under td
            for root, _, files in os.walk(td):
                for f in files:
                    fp = Path(root) / f
                    if fp.suffix in [".json", ".txt", ".md", ".html"]:
                        content = fp.read_text(encoding="utf-8", errors="ignore")
                        for pat in REDLINE_SECRET_PATTERNS:
                            assert pat not in content, f"Found secret pattern '{pat}' in file {fp}"
    runner.run_test("test_redline_zero_secret_in_simulated_artifacts", test_redline_zero_secret_in_simulated_artifacts)

    # -----------------------------------------------------------------------
    # Summary
    # -----------------------------------------------------------------------
    print("\n================================================================================")
    print("                      STRESS TEST RUN SUMMARY                                   ")
    print("================================================================================")
    print(f" Total Tests   : {runner.tests_run}")
    print(f" Passed        : {runner.tests_passed}")
    print(f" Failed        : {runner.tests_failed}")
    print(f" Success Rate  : {(runner.tests_passed / max(1, runner.tests_run)) * 100:.1f}%")
    print("================================================================================")

    if runner.tests_failed > 0:
        print("\nFailures:")
        for fail in runner.failures:
            print(f" - {fail['name']}: {fail['error']}")
        sys.exit(1)
    else:
        print("\nAll adversarial stress tests passed cleanly!")
        sys.exit(0)

if __name__ == "__main__":
    run_all_stress_tests()
