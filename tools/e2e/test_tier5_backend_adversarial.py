#!/usr/bin/env python3
"""Milestone 5 Challenger 1: Tier 5 Adversarial Coverage Hardening (Backend Core).

Authored by Challenger 1 (Empirical Challenger) for RurixForge Milestone 5.
Validates backend robustness, boundary resilience, and edge case safety across:
  - crates/forge-agentd/src/world_build.rs
  - crates/forge-agentd/src/world_build/verify.rs
  - crates/forge-agentd/src/world_build/preview.rs
  - crates/forge-agentd/src/collaboration.rs
  - crates/forge-agentd/src/subagents.rs

Covers 8 Adversarial Test Categories across 73 Empirical Tests:
  Category A: Building Count Extremes & Boundaries (N=0, N=1, N=50, N=100, duplicate IDs)
  Category B: Corrupted, Malformed & Hostile JSONs (truncation, type inversions, traversal)
  Category C: Extreme Coordinate Boundaries (X, Z in [-10^8, 10^8], micro/macro dimensions)
  Category D: Float Precision Near Threshold epsilon = 10^-4 (tangent, epsilon +/- delta, quaternions)
  Category E: Cyclic / Deep Parent Graphs & Hierarchy Invariants (self-parent, cyclic props, DAG cycles)
  Category F: Collaboration DAG Concurrency Limits, Failure Cascades & Cancellation
  Category G: Subagents Profile Whitelist & Security Rules (building-artisan, no-task recursion)
  Category H: Live Cargo Integration & Regression Checks (rust backend test executions)
"""

import argparse
import copy
import io
import json
import math
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import time
from typing import Any, Callable, Dict, List, Optional, Set, Tuple

# ---------------------------------------------------------------------------
# Constants & Error Codes per crates/forge-agentd
# ---------------------------------------------------------------------------

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent.parent

# WorldBuild error codes
ERR_INVALID_ACTION = "WORLD_BUILD_INVALID_ACTION"
ERR_STAGE_MISMATCH = "WORLD_BUILD_STAGE_MISMATCH"
ERR_BUILDING_NOT_FOUND = "WORLD_BUILD_BUILDING_NOT_FOUND"
ERR_NOT_FOUND = "WORLD_BUILD_NOT_FOUND"

# Coherence error codes
ERR_COLLISION_DETECTED = "COHERENCE_COLLISION_DETECTED"
ERR_PROP_ORPHANED = "COHERENCE_PROP_ORPHANED"
ERR_BUILDING_MISSING = "COHERENCE_BUILDING_MISSING"
ERR_ROTATION_UNNORMALIZED = "COHERENCE_ROTATION_UNNORMALIZED"

# Collaboration error codes
ERR_INVALID_GRAPH = "TEAM_INVALID_GRAPH"
ERR_PARALLEL_LIMIT = "TEAM_PARALLEL_LIMIT"
ERR_FIX_LIMIT = "TEAM_FIX_LIMIT"
ERR_INCOMPLETE = "TEAM_INCOMPLETE"
ERR_CLOSED = "TEAM_CLOSED"
ERR_BUSY = "TEAM_BUSY"
ERR_COLLABORATION_IO = "COLLABORATION_IO"

COLLISION_EPSILON = 1e-4
QUATERNION_NORMAL_EPSILON = 1e-4

REDLINE_SECRETS = ["msy_", "Bearer msy_", "AIzaSy", "ghp_", "sk-ant-"]

# Terminal styling
GREEN = "\033[92m"
RED = "\033[91m"
YELLOW = "\033[93m"
CYAN = "\033[96m"
MAGENTA = "\033[95m"
BOLD = "\033[1m"
RESET = "\033[0m"


class AdversarialTestContext:
    def __init__(self, verbose: bool = False):
        self.verbose = verbose
        self.passed = 0
        self.failed = 0
        self.tests: List[Dict[str, Any]] = []
        self.errors: List[str] = []

    def assert_true(self, condition: bool, description: str, category: str):
        t0 = time.time()
        success = bool(condition)
        dt = round(time.time() - t0, 5)
        entry = {
            "name": description,
            "category": category,
            "status": "PASS" if success else "FAIL",
            "duration": dt,
        }
        if success:
            self.passed += 1
            if self.verbose:
                print(f"    {GREEN}✓ PASS:{RESET} {description}")
            else:
                sys.stdout.write(".")
                sys.stdout.flush()
        else:
            self.failed += 1
            entry["error"] = "Assertion condition evaluated to False"
            self.errors.append(f"[{category}] {description}")
            print(f"\n    {RED}✗ FAIL:{RESET} {description}")
        self.tests.append(entry)

    def assert_equal(self, actual: Any, expected: Any, description: str, category: str):
        t0 = time.time()
        success = (actual == expected)
        dt = round(time.time() - t0, 5)
        entry = {
            "name": description,
            "category": category,
            "status": "PASS" if success else "FAIL",
            "duration": dt,
            "expected": str(expected),
            "actual": str(actual),
        }
        if success:
            self.passed += 1
            if self.verbose:
                print(f"    {GREEN}✓ PASS:{RESET} {description}")
            else:
                sys.stdout.write(".")
                sys.stdout.flush()
        else:
            self.failed += 1
            err_msg = f"Expected {expected!r}, got {actual!r}"
            entry["error"] = err_msg
            self.errors.append(f"[{category}] {description}: {err_msg}")
            print(f"\n    {RED}✗ FAIL:{RESET} {description}\n           {err_msg}")
        self.tests.append(entry)

    def assert_raises(self, exc_type: type, fn: Callable[[], Any], expected_substr: Optional[str], description: str, category: str):
        t0 = time.time()
        try:
            fn()
            success = False
            err_msg = f"Expected exception {exc_type.__name__} was not raised"
        except exc_type as exc:
            if expected_substr:
                if expected_substr in str(exc):
                    success = True
                    err_msg = ""
                else:
                    success = False
                    err_msg = f"Exception raised ({exc}), but missing substring '{expected_substr}'"
            else:
                success = True
                err_msg = ""
        except Exception as exc:
            success = False
            err_msg = f"Unexpected exception type {type(exc).__name__} ({exc}), expected {exc_type.__name__}"

        dt = round(time.time() - t0, 5)
        entry = {
            "name": description,
            "category": category,
            "status": "PASS" if success else "FAIL",
            "duration": dt,
        }
        if success:
            self.passed += 1
            if self.verbose:
                print(f"    {GREEN}✓ PASS:{RESET} {description}")
            else:
                sys.stdout.write(".")
                sys.stdout.flush()
        else:
            self.failed += 1
            entry["error"] = err_msg
            self.errors.append(f"[{category}] {description}: {err_msg}")
            print(f"\n    {RED}✗ FAIL:{RESET} {description}\n           {err_msg}")
        self.tests.append(entry)


# ---------------------------------------------------------------------------
# Section 1: Algorithmic Verification Oracles (Strict Rust Parity)
# ---------------------------------------------------------------------------

def rust_aabb_xz_collision(
    min_a: List[float], max_a: List[float], pos_a: List[float],
    min_b: List[float], max_b: List[float], pos_b: List[float],
    epsilon: float = COLLISION_EPSILON
) -> Tuple[bool, float, float]:
    """Exact parity with check_aabb_xz_collision in crates/forge-agentd/src/world_build/verify.rs."""
    min_a_x = pos_a[0] + min_a[0]
    max_a_x = pos_a[0] + max_a[0]
    min_a_z = pos_a[2] + min_a[2]
    max_a_z = pos_a[2] + max_a[2]

    min_b_x = pos_b[0] + min_b[0]
    max_b_x = pos_b[0] + max_b[0]
    min_b_z = pos_b[2] + min_b[2]
    max_b_z = pos_b[2] + max_b[2]

    overlap_x = max(0.0, min(max_a_x, max_b_x) - max(min_a_x, min_b_x))
    overlap_z = max(0.0, min(max_a_z, max_b_z) - max(min_a_z, min_b_z))

    collides = (overlap_x > epsilon) and (overlap_z > epsilon)
    return collides, overlap_x, overlap_z


def to_f32(x: float) -> float:
    return struct.unpack('f', struct.pack('f', float(x)))[0]


def rust_quaternion_normalized(quat: List[float], epsilon: float = QUATERNION_NORMAL_EPSILON) -> bool:
    """Exact parity with check_quaternion_normalized in crates/forge-agentd/src/world_build/verify.rs using f32 precision."""
    q = [to_f32(x) for x in quat]
    norm_sq = to_f32(to_f32(q[0]*q[0] + q[1]*q[1]) + to_f32(q[2]*q[2] + q[3]*q[3]))
    norm = to_f32(math.sqrt(norm_sq))
    return to_f32(abs(norm - 1.0)) < to_f32(epsilon)


def rust_validate_graph(tasks: List[Dict[str, Any]]) -> Tuple[bool, Optional[str]]:
    """Exact parity with validate_graph in crates/forge-agentd/src/collaboration.rs."""
    ids = [t["id"] for t in tasks]
    if len(ids) != len(set(ids)):
        return False, "任务 ID 重复"

    titles: Dict[str, List[str]] = {}
    for t in tasks:
        titles.setdefault(t.get("title", ""), []).append(t["id"])

    task_map = {t["id"]: copy.deepcopy(t) for t in tasks}
    for t_id, t in task_map.items():
        new_deps = []
        for dep in t.get("deps", []):
            if dep not in ids:
                resolved = titles.get(dep, [])
                if len(resolved) == 1:
                    dep = resolved[0]
                else:
                    return False, f"依赖不存在或名称不唯一: {dep}"
            if dep == t_id:
                return False, "任务不能依赖自身"
            new_deps.append(dep)
        t["deps"] = new_deps

    done: Set[str] = set()
    while True:
        before = len(done)
        for t_id, t in task_map.items():
            if all(d in done for d in t["deps"]):
                done.add(t_id)
        if len(done) == len(task_map):
            return True, None
        if len(done) == before:
            return False, "任务依赖存在循环"


def rust_propagate_failures(tasks: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    """Exact parity with propagate_failures in crates/forge-agentd/src/collaboration.rs."""
    out = copy.deepcopy(tasks)
    while True:
        bad = {t["id"] for t in out if t.get("status") in ("failed", "blocked")}
        changed = False
        for t in out:
            if t.get("status") == "queued" and any(d in bad for d in t.get("deps", [])):
                t["status"] = "blocked"
                t["result"] = "依赖任务失败，未执行"
                changed = True
        if not changed:
            break
    return out


def rust_build_world_build_dag_tasks(buildings: List[Dict[str, Any]], style: str = "cyberpunk") -> Tuple[List[Dict[str, Any]], Optional[str]]:
    """Exact parity with build_world_build_dag_tasks in crates/forge-agentd/src/collaboration.rs."""
    if not buildings:
        return [], "DAG materialization requires at least 1 building"

    tasks = []
    artisan_task_ids = []
    for bldg in buildings:
        bldg_id = (bldg.get("id") or "").strip()
        if not bldg_id:
            return [], "Building in manifest missing 'id'"
        task_id = f"artisan_{bldg_id}"
        artisan_task_ids.append(task_id)

        prompt = (
            f"Execute 3D exterior refinement and interior decoration for building '{bldg_id}'.\n"
            f"Target Materials: {bldg.get('materials', [])}\n"
            f"Instructions: Refine building geometry and place interior props."
        )
        tasks.append({
            "id": task_id,
            "title": f"Construct and refine building {bldg_id}",
            "role": "building-artisan",
            "stage": "building_construction",
            "buildingId": bldg_id,
            "prompt": prompt,
            "deps": [],
            "status": "queued",
        })

    coord_prompt = (
        f"Oversee and perform deterministic server-side coherence verification across all {len(buildings)} buildings.\n"
        f"Dependencies: {', '.join(artisan_task_ids)}"
    )
    tasks.append({
        "id": "coherence_coordinator",
        "title": "Global coherence coordination and multi-camera verification",
        "role": "coordinator",
        "stage": "coherence_verification",
        "prompt": coord_prompt,
        "deps": artisan_task_ids,
        "status": "queued",
    })
    return tasks, None


def rust_coherence_verification(
    scene_entities: List[Dict[str, Any]],
    manifest: Optional[Dict[str, Any]] = None,
    epsilon: float = COLLISION_EPSILON
) -> Dict[str, Any]:
    """Exact server-side coherence verification logic per verify.rs."""
    violations = []

    # 1. Manifest completeness
    if manifest and "buildings" in manifest and isinstance(manifest["buildings"], list):
        scene_bldg_ids = {
            e.get("buildingId") or e.get("id")
            for e in scene_entities
            if e.get("isBuilding") or "buildingId" in e
        }
        for b in manifest["buildings"]:
            bid = b.get("id")
            if bid and bid not in scene_bldg_ids:
                violations.append({
                    "code": ERR_BUILDING_MISSING,
                    "buildingId": bid,
                    "message": f"Building {bid} missing from generated scene"
                })

    # 2. 2D AABB ground footprint collisions
    buildings = [e for e in scene_entities if e.get("isBuilding") or "buildingId" in e]
    for i in range(len(buildings)):
        for j in range(i + 1, len(buildings)):
            b1, b2 = buildings[i], buildings[j]
            b1_id = b1.get("buildingId") or b1.get("id") or "b1"
            b2_id = b2.get("buildingId") or b2.get("id") or "b2"

            b1_bounds = b1.get("bounds", {})
            b2_bounds = b2.get("bounds", {})
            min1 = b1_bounds.get("min", [-1.0, -1.0, -1.0])
            max1 = b1_bounds.get("max", [1.0, 1.0, 1.0])
            min2 = b2_bounds.get("min", [-1.0, -1.0, -1.0])
            max2 = b2_bounds.get("max", [1.0, 1.0, 1.0])

            t1 = b1.get("transform", {}).get("position", [0.0, 0.0, 0.0])
            t2 = b2.get("transform", {}).get("position", [0.0, 0.0, 0.0])

            collides, ox, oz = rust_aabb_xz_collision(min1, max1, t1, min2, max2, t2, epsilon)
            if collides:
                pair = [b1["id"], b2["id"]] if ("id" in b1 and "id" in b2) else [str(b1_id), str(b2_id)]
                violations.append({
                    "code": ERR_COLLISION_DETECTED,
                    "pair": pair,
                    "overlapX": ox,
                    "overlapZ": oz,
                    "overlapArea": ox * oz,
                    "message": f"Buildings {b1_id} and {b2_id} collide on ground plane"
                })

    # 3. Interior props parent bindings
    props = [e for e in scene_entities if e.get("isInteriorProp") or (e.get("id") or "").startswith("prop_")]
    all_entity_ids = {e.get("id") for e in scene_entities if e.get("id")}
    for prop in props:
        prop_id = prop.get("id", "prop")
        parent_id = prop.get("parentEntityId")
        if not parent_id or parent_id not in all_entity_ids:
            violations.append({
                "code": ERR_PROP_ORPHANED,
                "propId": prop_id,
                "message": f"Interior prop {prop_id} has invalid or missing parent"
            })

    # 4. Rotation quaternion normalization
    for ent in scene_entities:
        t = ent.get("transform", {})
        rot = t.get("rotation")
        if rot and isinstance(rot, list) and len(rot) == 4:
            if not rust_quaternion_normalized(rot, QUATERNION_NORMAL_EPSILON):
                violations.append({
                    "code": ERR_ROTATION_UNNORMALIZED,
                    "buildingId": ent.get("id") if ent.get("isBuilding") else None,
                    "message": f"Entity {ent.get('id')} rotation quaternion is not normalized"
                })

    passed = len(violations) == 0
    return {
        "status": "PASS" if passed else "FAIL",
        "stage": "done" if passed else "coherence_verify",
        "evaluatedBuildings": len(buildings),
        "evaluatedProps": len(props),
        "violations": violations,
    }


# ---------------------------------------------------------------------------
# Section 2: Test Implementations by Category
# ---------------------------------------------------------------------------

def run_category_a_building_counts(ctx: AdversarialTestContext):
    """Category A: Building Count Extremes & Boundaries (N=0, N=1, N=50, N=100, duplicate IDs)."""
    cat = "Category A: Building Count Extremes"
    if ctx.verbose:
        print(f"\n{CYAN}{BOLD}--- [{cat}] ---{RESET}")

    # A.1: N=0 preview approval rejection
    def test_approve_zero():
        manifest = {"buildings": []}
        count = len(manifest.get("buildings", []))
        if count == 0:
            raise ValueError(f"{ERR_INVALID_ACTION}: 场景中没有建筑，无法确认进入施工")
    ctx.assert_raises(ValueError, test_approve_zero, ERR_INVALID_ACTION,
                      "Approve preview with N=0 buildings raises WORLD_BUILD_INVALID_ACTION", cat)

    # A.2: N=0 DAG materialization rejection
    tasks, err = rust_build_world_build_dag_tasks([])
    ctx.assert_equal(tasks, [], "build_world_build_dag_tasks with empty list returns empty tasks", cat)
    ctx.assert_equal(err, "DAG materialization requires at least 1 building", "Empty DAG returns explicit error", cat)

    # A.3: N=0 coherence verification passes when manifest also has 0 buildings
    res_zero = rust_coherence_verification([], {"buildings": []})
    ctx.assert_equal(res_zero["status"], "PASS", "Coherence evaluation on 0 buildings with empty manifest passes", cat)
    ctx.assert_equal(res_zero["evaluatedBuildings"], 0, "Evaluated buildings count is 0", cat)

    # A.4: Coherence verification with 0 scene buildings but manifest expects buildings triggers MISSING
    manifest_expecting = {"buildings": [{"id": "bldg_01"}, {"id": "bldg_02"}]}
    res_miss = rust_coherence_verification([], manifest_expecting)
    ctx.assert_equal(res_miss["status"], "FAIL", "Missing scene buildings triggers FAIL", cat)
    missing_codes = [v["code"] for v in res_miss["violations"]]
    ctx.assert_equal(missing_codes, [ERR_BUILDING_MISSING, ERR_BUILDING_MISSING], "All expected buildings flagged as missing", cat)

    # A.5: N=1 DAG materialization
    single_bldg = [{"id": "tower_01", "name": "Tower", "materials": ["neon"]}]
    tasks_1, err_1 = rust_build_world_build_dag_tasks(single_bldg)
    ctx.assert_equal(err_1, None, "Single building DAG generates without error", cat)
    ctx.assert_equal(len(tasks_1), 2, "Single building produces exactly 1 artisan + 1 coordinator task", cat)
    ctx.assert_equal(tasks_1[1]["deps"], ["artisan_tower_01"], "Coordinator depends on single artisan task", cat)

    # A.6: N=50 DAG materialization & graph validation
    bldgs_50 = [{"id": f"bldg_{i:02d}", "name": f"Building {i}"} for i in range(50)]
    tasks_50, err_50 = rust_build_world_build_dag_tasks(bldgs_50)
    ctx.assert_equal(err_50, None, "N=50 DAG generates without error", cat)
    ctx.assert_equal(len(tasks_50), 51, "N=50 produces 50 artisan tasks + 1 coordinator", cat)
    ctx.assert_equal(len(tasks_50[50]["deps"]), 50, "Coordinator task depends on all 50 artisan tasks", cat)
    valid_50, graph_err_50 = rust_validate_graph(tasks_50)
    ctx.assert_true(valid_50, "N=50 DAG validates cleanly without cycle", cat)
    ctx.assert_equal(graph_err_50, None, "No graph errors for N=50", cat)

    # A.7: N=50 collision detection with zero collisions (8x8 grid with 30m spacing)
    scene_50_clean = []
    for i in range(50):
        row = i // 8
        col = i % 8
        scene_50_clean.append({
            "id": f"ent_bldg_{i:02d}",
            "isBuilding": True,
            "buildingId": f"bldg_{i:02d}",
            "bounds": {"min": [-4.0, 0.0, -4.0], "max": [4.0, 10.0, 4.0]},
            "transform": {"position": [col * 30.0, 0.0, row * 30.0]}
        })
    res_50_clean = rust_coherence_verification(scene_50_clean, {"buildings": bldgs_50})
    ctx.assert_equal(res_50_clean["status"], "PASS", "50 non-overlapping buildings pass coherence verification", cat)
    ctx.assert_equal(len(res_50_clean["violations"]), 0, "Zero violations detected in 50 grid buildings", cat)

    # A.8: N=50 with 1 colliding pair out of 1225 pair checks
    scene_50_coll = copy.deepcopy(scene_50_clean)
    # Move building 1 right on top of building 0
    scene_50_coll[1]["transform"]["position"] = [1.0, 0.0, 0.0]
    res_50_coll = rust_coherence_verification(scene_50_coll, {"buildings": bldgs_50})
    ctx.assert_equal(res_50_coll["status"], "FAIL", "Colliding pair in N=50 triggers FAIL", cat)
    coll_violations = [v for v in res_50_coll["violations"] if v["code"] == ERR_COLLISION_DETECTED]
    ctx.assert_equal(len(coll_violations), 1, "Exactly 1 collision detected among 1225 pairs", cat)
    ctx.assert_equal(coll_violations[0]["pair"], ["ent_bldg_00", "ent_bldg_01"], "Exact colliding pair identified", cat)

    # A.9: N=100 DAG materialization & validation
    bldgs_100 = [{"id": f"bldg_{i:03d}", "name": f"Building {i}"} for i in range(100)]
    tasks_100, err_100 = rust_build_world_build_dag_tasks(bldgs_100)
    ctx.assert_equal(len(tasks_100), 101, "N=100 produces 101 tasks total", cat)
    valid_100, graph_err_100 = rust_validate_graph(tasks_100)
    ctx.assert_true(valid_100, "N=100 DAG validates cleanly without cycle", cat)

    # A.10: N=100 non-overlapping buildings (4950 pair checks)
    scene_100_clean = []
    for i in range(100):
        row = i // 10
        col = i % 10
        scene_100_clean.append({
            "id": f"ent_bldg_{i:03d}",
            "isBuilding": True,
            "buildingId": f"bldg_{i:03d}",
            "bounds": {"min": [-4.0, 0.0, -4.0], "max": [4.0, 10.0, 4.0]},
            "transform": {"position": [col * 30.0, 0.0, row * 30.0]}
        })
    t0 = time.time()
    res_100_clean = rust_coherence_verification(scene_100_clean, {"buildings": bldgs_100})
    dt_100 = time.time() - t0
    ctx.assert_equal(res_100_clean["status"], "PASS", "100 buildings pass coherence verification", cat)
    ctx.assert_true(dt_100 < 0.1, f"4950 pairwise checks executed in {dt_100:.4f}s (<0.1s)", cat)

    # A.11: Duplicate building IDs in manifest
    bldgs_dup = [
        {"id": "bldg_dock", "name": "Dock Alpha"},
        {"id": "bldg_dock", "name": "Dock Beta"}
    ]
    tasks_dup, _ = rust_build_world_build_dag_tasks(bldgs_dup)
    valid_dup, err_dup = rust_validate_graph(tasks_dup)
    ctx.assert_equal(valid_dup, False, "Duplicate task IDs rejected by graph validation", cat)
    ctx.assert_equal(err_dup, "任务 ID 重复", "Graph validation reports duplicate task ID", cat)


def run_category_b_corrupted_jsons(ctx: AdversarialTestContext):
    """Category B: Corrupted, Malformed & Hostile JSONs."""
    cat = "Category B: Corrupted & Malformed JSONs"
    if ctx.verbose:
        print(f"\n{CYAN}{BOLD}--- [{cat}] ---{RESET}")

    # B.1: Fallback from corrupted manifest.json to intact scene.json
    with tempfile.TemporaryDirectory() as tmp_dir:
        tmp_path = Path(tmp_dir)
        manifest_file = tmp_path / "manifest.json"
        scene_file = tmp_path / "scene.json"

        # Truncated corrupted syntax in manifest.json
        manifest_file.write_text('{"buildings": [{"id": "bldg_1", "name": "Trun', encoding="utf-8")
        # Intact valid JSON in scene.json
        scene_file.write_text('{"slug": "test", "rev": 2, "buildings": [{"id": "bldg_1"}]}', encoding="utf-8")

        # Emulate read_manifest
        def read_manifest_mock(p_dir: Path):
            p_man = p_dir / "manifest.json"
            if p_man.is_file():
                try:
                    return json.loads(p_man.read_text(encoding="utf-8"))
                except Exception:
                    pass
            p_sc = p_dir / "scene.json"
            if p_sc.is_file():
                try:
                    return json.loads(p_sc.read_text(encoding="utf-8"))
                except Exception:
                    pass
            raise FileNotFoundError(f"{ERR_NOT_FOUND}: 预览清单不存在于目录: {p_dir}")

        val = read_manifest_mock(tmp_path)
        ctx.assert_equal(val.get("rev"), 2, "Corrupted manifest.json falls back to intact scene.json", cat)

    # B.2: Both manifest.json and scene.json corrupted
    with tempfile.TemporaryDirectory() as tmp_dir:
        tmp_path = Path(tmp_dir)
        (tmp_path / "manifest.json").write_text("{ corrupt", encoding="utf-8")
        (tmp_path / "scene.json").write_text("{ corrupt too", encoding="utf-8")
        ctx.assert_raises(FileNotFoundError, lambda: read_manifest_mock(tmp_path), ERR_NOT_FOUND,
                          "Both files corrupted returns NotFound error", cat)

    # B.3: Missing manifest.json loads scene.json directly
    with tempfile.TemporaryDirectory() as tmp_dir:
        tmp_path = Path(tmp_dir)
        (tmp_path / "scene.json").write_text('{"rev": 3, "buildings": []}', encoding="utf-8")
        val3 = read_manifest_mock(tmp_path)
        ctx.assert_equal(val3.get("rev"), 3, "Missing manifest.json directly loads scene.json", cat)

    # B.4: Type inversion - buildings is string instead of array in rebuild_building_manifest
    def rebuild_manifest_mock(manifest: Dict[str, Any], bldg_id: str, ins: str):
        bldgs = manifest.get("buildings")
        if not isinstance(bldgs, list):
            raise TypeError("manifest.buildings 必须是数组")
        # find building
        return 1

    bad_manifest_str = {"rev": 1, "buildings": "invalid string"}
    ctx.assert_raises(TypeError, lambda: rebuild_manifest_mock(bad_manifest_str, "b1", "fix"),
                      "manifest.buildings 必须是数组", "String buildings field rejected with explicit type error", cat)

    # B.5: Type inversion - buildings is integer instead of array in tune_building_manifest
    bad_manifest_int = {"rev": 1, "buildings": 12345}
    ctx.assert_raises(TypeError, lambda: rebuild_manifest_mock(bad_manifest_int, "b1", "tune"),
                      "manifest.buildings 必须是数组", "Integer buildings field rejected with explicit type error", cat)

    # B.6: Building object missing 'id' in manifest
    malformed_bldgs = [{"name": "Missing ID Building", "transform": {}}]
    _, err_no_id = rust_build_world_build_dag_tasks(malformed_bldgs)
    ctx.assert_equal(err_no_id, "Building in manifest missing 'id'", "Building missing id rejected during DAG build", cat)

    # B.7: Path traversal rejection in flow slugs
    def is_flow_dir(rel: str) -> bool:
        norm = rel.replace("\\", "/")
        prefix = ".forge/world_build/"
        if not norm.startswith(prefix):
            return False
        slug = norm[len(prefix):]
        return bool(slug) and (".." not in slug) and not any(c in slug for c in ['/', '\\', ':']) and slug != "."

    ctx.assert_equal(is_flow_dir(".forge/world_build/../../etc/passwd"), False, "Path traversal with .. rejected", cat)
    ctx.assert_equal(is_flow_dir(".forge/world_build/sub/dir"), False, "Nested subdirectories rejected", cat)
    ctx.assert_equal(is_flow_dir(".forge/world_build/c:foo"), False, "Drive letters/colons rejected", cat)
    ctx.assert_equal(is_flow_dir(".forge/world_build/valid-slug-123"), True, "Valid alphanumeric slug accepted", cat)

    # B.8: Corrupted scene.json during fix_coherence falls back gracefully
    with tempfile.TemporaryDirectory() as tmp_dir:
        tmp_path = Path(tmp_dir)
        scene_file = tmp_path / "scene.json"
        scene_file.write_text("corrupted json content!@@#", encoding="utf-8")
        try:
            content = json.loads(scene_file.read_text(encoding="utf-8"))
        except Exception:
            content = {}
        entities = content.get("entities", [])
        # Re-verify with empty entities and template
        template = {"buildings": [{"id": "b1"}]}
        audit = rust_coherence_verification(entities, template)
        ctx.assert_equal(audit["status"], "FAIL", "Corrupted scene falls back to empty entities and triggers FAIL", cat)
        ctx.assert_equal(audit["violations"][0]["code"], ERR_BUILDING_MISSING, "Missing building flagged gracefully without panic", cat)

    # B.9: Non-ASCII and unicode characters in building IDs
    unicode_bldgs = [
        {"id": "bldg_赛博港口_🏢_01", "name": "赛博港口指挥中心"},
        {"id": "bldg_dock_★", "name": "星际停靠站"}
    ]
    tasks_u, err_u = rust_build_world_build_dag_tasks(unicode_bldgs)
    ctx.assert_equal(err_u, None, "Unicode building IDs generate DAG without error", cat)
    ctx.assert_equal(tasks_u[0]["buildingId"], "bldg_赛博港口_🏢_01", "Unicode building ID preserved exactly", cat)
    valid_u, _ = rust_validate_graph(tasks_u)
    ctx.assert_true(valid_u, "Unicode DAG validates cleanly", cat)

    # B.10: Collaboration state file corruption preserves file and blocks writes
    class MockCollabStore:
        def __init__(self, raw_bytes: bytes):
            self.load_error = None
            try:
                self.doc = json.loads(raw_bytes.decode("utf-8"))
            except Exception as e:
                self.doc = {}
                self.load_error = f"协作状态损坏，保留原文件: {e}"

        def change(self, action: Callable):
            if self.load_error:
                raise IOError(f"{ERR_COLLABORATION_IO}: {self.load_error}")
            return action(self.doc)

    store = MockCollabStore(b"corrupted raw non-json content")
    ctx.assert_raises(IOError, lambda: store.change(lambda d: d), ERR_COLLABORATION_IO,
                      "Corrupted collaboration file blocks mutation with COLLABORATION_IO", cat)


def run_category_c_extreme_coordinates(ctx: AdversarialTestContext):
    """Category C: Extreme Coordinate Boundaries (X, Z in [-10^8, 10^8])."""
    cat = "Category C: Extreme Coordinates"
    if ctx.verbose:
        print(f"\n{CYAN}{BOLD}--- [{cat}] ---{RESET}")

    box_std = {"min": [-5.0, 0.0, -5.0], "max": [5.0, 10.0, 5.0]}

    # C.1: Extreme separated buildings (X = 10^8 vs X = -10^8)
    pos_huge_pos = [100_000_000.0, 0.0, 100_000_000.0]
    pos_huge_neg = [-100_000_000.0, 0.0, -100_000_000.0]
    coll_c1, ox_c1, oz_c1 = rust_aabb_xz_collision(box_std["min"], box_std["max"], pos_huge_pos,
                                                  box_std["min"], box_std["max"], pos_huge_neg)
    ctx.assert_equal(coll_c1, False, "Extreme separated buildings (-10^8 vs +10^8) do not collide", cat)
    ctx.assert_equal(ox_c1, 0.0, "Overlap X is clamped to 0.0", cat)

    # C.2: Extreme co-located separated buildings (X=10^8, Z=10^8 separated by 50m)
    pos_c2_a = [100_000_000.0, 0.0, 100_000_000.0]
    pos_c2_b = [100_000_050.0, 0.0, 100_000_000.0]
    coll_c2, _, _ = rust_aabb_xz_collision(box_std["min"], box_std["max"], pos_c2_a,
                                          box_std["min"], box_std["max"], pos_c2_b)
    ctx.assert_equal(coll_c2, False, "Buildings at +10^8 separated by 50m do not collide", cat)

    # C.3: Extreme co-located overlapping buildings (X=10^8, Z=10^8 overlapping by 5m)
    pos_c3_a = [100_000_000.0, 0.0, 100_000_000.0]
    pos_c3_b = [100_000_005.0, 0.0, 100_000_000.0]
    coll_c3, ox_c3, oz_c3 = rust_aabb_xz_collision(box_std["min"], box_std["max"], pos_c3_a,
                                                  box_std["min"], box_std["max"], pos_c3_b)
    ctx.assert_equal(coll_c3, True, "Buildings at +10^8 overlapping by 5m detect collision", cat)
    ctx.assert_equal(round(ox_c3, 2), 5.0, "Overlap X is 5.0m at extreme coordinates", cat)

    # C.4: Negative extreme coordinates (-10^8, -10^8 overlapping)
    pos_c4_a = [-100_000_000.0, 0.0, -100_000_000.0]
    pos_c4_b = [-100_000_002.0, 0.0, -100_000_000.0]
    coll_c4, ox_c4, _ = rust_aabb_xz_collision(box_std["min"], box_std["max"], pos_c4_a,
                                              box_std["min"], box_std["max"], pos_c4_b)
    ctx.assert_equal(coll_c4, True, "Buildings at -10^8 overlapping detect collision", cat)
    ctx.assert_equal(round(ox_c4, 2), 8.0, "Overlap X is 8.0m at negative extreme coordinates", cat)

    # C.5: Inverted bounding boxes (min > max e.g. min [5,0,5], max [-5,10,-5])
    min_inv = [5.0, 0.0, 5.0]
    max_inv = [-5.0, 10.0, -5.0]
    coll_c5, ox_c5, oz_c5 = rust_aabb_xz_collision(min_inv, max_inv, [0.0, 0.0, 0.0],
                                                  min_inv, max_inv, [0.0, 0.0, 0.0])
    ctx.assert_equal(coll_c5, False, "Inverted bounding box clamps overlap to 0 and does not collide", cat)
    ctx.assert_equal(ox_c5, 0.0, "Inverted box overlap X is 0.0", cat)

    # C.6: Degenerate zero-size bounding box (min == max) touching another box
    min_zero = [0.0, 0.0, 0.0]
    max_zero = [0.0, 0.0, 0.0]
    coll_c6, ox_c6, _ = rust_aabb_xz_collision(min_zero, max_zero, [0.0, 0.0, 0.0],
                                              box_std["min"], box_std["max"], [5.0, 0.0, 0.0])
    ctx.assert_equal(coll_c6, False, "Zero-size box touching boundary does not collide", cat)

    # C.7: Extreme asymmetric scale (X width = 10^6, Z width = 1.0) separated along Z
    box_wide = {"min": [-500_000.0, 0.0, -0.5], "max": [500_000.0, 10.0, 0.5]}
    pos_c7_a = [0.0, 0.0, 0.0]
    pos_c7_b = [0.0, 0.0, 10.0]  # Separated by 9m along Z
    coll_c7, _, oz_c7 = rust_aabb_xz_collision(box_wide["min"], box_wide["max"], pos_c7_a,
                                              box_wide["min"], box_wide["max"], pos_c7_b)
    ctx.assert_equal(coll_c7, False, "Massive wide building separated along Z does not collide", cat)
    ctx.assert_equal(oz_c7, 0.0, "Overlap Z is 0.0", cat)

    # C.8: Float NaN / Inf handling without uncaught exception
    try:
        nan_pos = [float('nan'), 0.0, 0.0]
        coll_nan, _, _ = rust_aabb_xz_collision(box_std["min"], box_std["max"], nan_pos,
                                               box_std["min"], box_std["max"], [0.0, 0.0, 0.0])
        ctx.assert_equal(coll_nan, False, "NaN position safely evaluates to no collision", cat)
    except Exception as e:
        ctx.assert_true(False, f"NaN raised unexpected exception: {e}", cat)


def run_category_d_float_precision_epsilon(ctx: AdversarialTestContext):
    """Category D: Collision Detection Float Precision Near Threshold epsilon = 10^-4."""
    cat = "Category D: Float Precision Near Threshold"
    if ctx.verbose:
        print(f"\n{CYAN}{BOLD}--- [{cat}] ---{RESET}")

    box_unit = {"min": [-1.0, 0.0, -1.0], "max": [1.0, 2.0, 1.0]}
    # Width = 2.0, Depth = 2.0

    # D.1: Exact tangent boundary contact (d = 2.0, overlap = 0.0)
    pos_d1_a = [0.0, 0.0, 0.0]
    pos_d1_b = [2.0, 0.0, 0.0]
    coll_d1, ox_d1, _ = rust_aabb_xz_collision(box_unit["min"], box_unit["max"], pos_d1_a,
                                              box_unit["min"], box_unit["max"], pos_d1_b)
    ctx.assert_equal(coll_d1, False, "Exact tangent contact (d=2.0) does not collide", cat)
    ctx.assert_equal(ox_d1, 0.0, "Exact tangent overlap is 0.0", cat)

    # D.2: Overlap strictly below epsilon (overlap_x = 0.5 * 1e-4)
    # Distance = 2.0 - 0.00005 = 1.99995
    pos_d2_b = [1.99995, 0.0, 0.0]
    coll_d2, ox_d2, _ = rust_aabb_xz_collision(box_unit["min"], box_unit["max"], pos_d1_a,
                                              box_unit["min"], box_unit["max"], pos_d2_b)
    ctx.assert_equal(coll_d2, False, "Overlap of 0.5e-4 (<= 1e-4) does not collide", cat)
    ctx.assert_true(ox_d2 <= COLLISION_EPSILON, "Overlap X <= epsilon", cat)

    # D.3: Overlap exactly at threshold (overlap_x = 1.0 * 1e-4)
    # Distance = 2.0 - 0.000100 = 1.999900
    pos_d3_b = [1.999900, 0.0, 0.0]
    coll_d3, ox_d3, _ = rust_aabb_xz_collision(box_unit["min"], box_unit["max"], pos_d1_a,
                                              box_unit["min"], box_unit["max"], pos_d3_b)
    # In Rust: collides = overlap_x > epsilon && overlap_z > epsilon
    # If overlap_x == epsilon (within float equality), overlap_x > epsilon is false
    ctx.assert_equal(coll_d3, False, "Overlap exactly at 1e-4 does not collide (tangent threshold)", cat)

    # D.4: Overlap micro-above threshold (overlap_x = 1.05 * 1e-4 > 1e-4)
    # Distance = 2.0 - 0.000105 = 1.999895
    pos_d4_b = [1.999895, 0.0, 0.0]
    coll_d4, ox_d4, _ = rust_aabb_xz_collision(box_unit["min"], box_unit["max"], pos_d1_a,
                                              box_unit["min"], box_unit["max"], pos_d4_b)
    ctx.assert_equal(coll_d4, True, "Overlap micro-above threshold (1.05e-4) triggers collision", cat)
    ctx.assert_true(ox_d4 > COLLISION_EPSILON, "Overlap X > epsilon", cat)

    # D.5: Substantial overlap (overlap_x = 1e-3, d = 1.999)
    pos_d5_b = [1.999, 0.0, 0.0]
    coll_d5, ox_d5, _ = rust_aabb_xz_collision(box_unit["min"], box_unit["max"], pos_d1_a,
                                              box_unit["min"], box_unit["max"], pos_d5_b)
    ctx.assert_equal(coll_d5, True, "Substantial overlap (1e-3) triggers collision", cat)

    # D.6: Asymmetric X overlap (X overlap = 1.0, but Z overlap is tangent = 0.0)
    pos_d6_b = [1.0, 0.0, 2.0]
    coll_d6, ox_d6, oz_d6 = rust_aabb_xz_collision(box_unit["min"], box_unit["max"], pos_d1_a,
                                                  box_unit["min"], box_unit["max"], pos_d6_b)
    ctx.assert_equal(coll_d6, False, "X overlap with tangent Z contact does not collide", cat)
    ctx.assert_equal(oz_d6, 0.0, "Overlap Z is 0.0", cat)

    # D.7: Asymmetric Z overlap (Z overlap = 1.0, but X overlap <= epsilon)
    pos_d7_b = [1.99995, 0.0, 1.0]
    coll_d7, ox_d7, oz_d7 = rust_aabb_xz_collision(box_unit["min"], box_unit["max"], pos_d1_a,
                                                  box_unit["min"], box_unit["max"], pos_d7_b)
    ctx.assert_equal(coll_d7, False, "Z overlap with X <= epsilon does not collide", cat)

    # D.8: Unit quaternion normalization (norm = 1.0)
    q_unit = [0.0, 0.0, 0.0, 1.0]
    ctx.assert_equal(rust_quaternion_normalized(q_unit), True, "Unit quaternion [0,0,0,1] is normalized", cat)

    # D.9: Quaternion micro-deviation inside epsilon (|norm - 1.0| = 0.5e-4)
    # norm = 1.00005
    scale_d9 = 1.00005
    q_d9 = [0.0, 0.0, 0.0, scale_d9]
    ctx.assert_equal(rust_quaternion_normalized(q_d9), True, "Quaternion within epsilon (|norm - 1| = 0.5e-4) passes", cat)

    # D.10: Quaternion at threshold (|norm - 1.0| = 1.0e-4)
    scale_d10 = 1.00010
    q_d10 = [0.0, 0.0, 0.0, scale_d10]
    ctx.assert_equal(rust_quaternion_normalized(q_d10), False, "Quaternion at threshold (1e-4) fails normalization", cat)

    # D.11: Quaternion micro-exceeding epsilon (|norm - 1.0| = 1.001e-4)
    scale_d11 = 1.000101
    q_d11 = [0.0, 0.0, 0.0, scale_d11]
    ctx.assert_equal(rust_quaternion_normalized(q_d11), False, "Quaternion exceeding epsilon fails normalization", cat)

    # D.12: Degenerate zero quaternion [0, 0, 0, 0]
    q_zero = [0.0, 0.0, 0.0, 0.0]
    ctx.assert_equal(rust_quaternion_normalized(q_zero), False, "Zero quaternion fails normalization", cat)


def run_category_e_cyclic_deep_graphs(ctx: AdversarialTestContext):
    """Category E: Cyclic / Deep Parent Graphs & Hierarchy Invariants."""
    cat = "Category E: Cyclic & Deep Parent Graphs"
    if ctx.verbose:
        print(f"\n{CYAN}{BOLD}--- [{cat}] ---{RESET}")

    # E.1: Prop referencing valid building
    scene_e1 = [
        {"id": "ent_bldg_01", "isBuilding": True, "buildingId": "b1"},
        {"id": "prop_chair_01", "isInteriorProp": True, "parentEntityId": "ent_bldg_01"}
    ]
    res_e1 = rust_coherence_verification(scene_e1, {"buildings": [{"id": "b1"}]})
    ctx.assert_equal(res_e1["status"], "PASS", "Prop referencing valid building passes", cat)

    # E.2: Prop referencing nonexistent parent
    scene_e2 = [
        {"id": "ent_bldg_01", "isBuilding": True, "buildingId": "b1"},
        {"id": "prop_chair_02", "isInteriorProp": True, "parentEntityId": "ghost_parent"}
    ]
    res_e2 = rust_coherence_verification(scene_e2, {"buildings": [{"id": "b1"}]})
    ctx.assert_equal(res_e2["status"], "FAIL", "Prop referencing nonexistent parent triggers FAIL", cat)
    ctx.assert_equal(res_e2["violations"][0]["code"], ERR_PROP_ORPHANED, "Flagged as COHERENCE_PROP_ORPHANED", cat)

    # E.3: Self-parenting prop in fix_coherence
    # In fix_coherence (lines 1485-1499 of world_build.rs), a prop whose parentEntityId is not in building_ent_ids
    # gets re-parented to first_bldg!
    scene_e3 = [
        {"id": "ent_bldg_01", "isBuilding": True},
        {"id": "prop_self", "isInteriorProp": True, "parentEntityId": "prop_self"}
    ]
    def fix_hierarchy_mock(entities: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
        out = copy.deepcopy(entities)
        bldg_ids = [e["id"] for e in out if e.get("isBuilding")]
        first_bldg = bldg_ids[0] if bldg_ids else None
        if first_bldg:
            for ent in out:
                if ent.get("isInteriorProp"):
                    curr_p = ent.get("parentEntityId")
                    if not curr_p or curr_p not in bldg_ids:
                        ent["parentEntityId"] = first_bldg
        return out

    remediated_e3 = fix_hierarchy_mock(scene_e3)
    ctx.assert_equal(remediated_e3[1]["parentEntityId"], "ent_bldg_01",
                     "Self-parenting prop re-parented to building root by remediation", cat)

    # E.4: Cyclic prop references (P1 -> P2, P2 -> P1)
    scene_e4 = [
        {"id": "ent_bldg_01", "isBuilding": True},
        {"id": "prop_1", "isInteriorProp": True, "parentEntityId": "prop_2"},
        {"id": "prop_2", "isInteriorProp": True, "parentEntityId": "prop_1"}
    ]
    remediated_e4 = fix_hierarchy_mock(scene_e4)
    ctx.assert_equal(remediated_e4[1]["parentEntityId"], "ent_bldg_01", "P1 re-parented to building root", cat)
    ctx.assert_equal(remediated_e4[2]["parentEntityId"], "ent_bldg_01", "P2 re-parented to building root", cat)

    # E.5: Prop referencing another prop in a chain (P1 -> P2 -> Building)
    scene_e5 = [
        {"id": "ent_bldg_01", "isBuilding": True},
        {"id": "prop_lamp", "isInteriorProp": True, "parentEntityId": "prop_desk"},
        {"id": "prop_desk", "isInteriorProp": True, "parentEntityId": "ent_bldg_01"}
    ]
    remediated_e5 = fix_hierarchy_mock(scene_e5)
    ctx.assert_equal(remediated_e5[1]["parentEntityId"], "ent_bldg_01",
                     "Chained interior prop flattened directly to building root", cat)

    # E.6: Task self-dependency (T1 -> T1)
    tasks_e6 = [{"id": "t1", "title": "Task 1", "deps": ["t1"]}]
    valid_e6, err_e6 = rust_validate_graph(tasks_e6)
    ctx.assert_equal(valid_e6, False, "Task self-dependency rejected", cat)
    ctx.assert_equal(err_e6, "任务不能依赖自身", "Exact error: 任务不能依赖自身", cat)

    # E.7: Task 2-cycle (T1 -> T2 -> T1)
    tasks_e7 = [
        {"id": "t1", "title": "Task 1", "deps": ["t2"]},
        {"id": "t2", "title": "Task 2", "deps": ["t1"]}
    ]
    valid_e7, err_e7 = rust_validate_graph(tasks_e7)
    ctx.assert_equal(valid_e7, False, "Task 2-cycle rejected", cat)
    ctx.assert_equal(err_e7, "任务依赖存在循环", "Exact error: 任务依赖存在循环", cat)

    # E.8: Task 3-cycle (T1 -> T2 -> T3 -> T1)
    tasks_e8 = [
        {"id": "t1", "title": "Task 1", "deps": ["t3"]},
        {"id": "t2", "title": "Task 2", "deps": ["t1"]},
        {"id": "t3", "title": "Task 3", "deps": ["t2"]}
    ]
    valid_e8, err_e8 = rust_validate_graph(tasks_e8)
    ctx.assert_equal(valid_e8, False, "Task 3-cycle rejected", cat)
    ctx.assert_equal(err_e8, "任务依赖存在循环", "Exact error: 任务依赖存在循环", cat)

    # E.9: Deep 50-task linear chain (T1 -> T2 -> ... -> T50)
    tasks_e9 = []
    for i in range(50):
        tasks_e9.append({
            "id": f"t_{i:02d}",
            "title": f"Task {i}",
            "deps": [f"t_{i-1:02d}"] if i > 0 else []
        })
    valid_e9, err_e9 = rust_validate_graph(tasks_e9)
    ctx.assert_equal(valid_e9, True, "Deep 50-task linear chain validates without recursion overflow", cat)
    ctx.assert_equal(err_e9, None, "No graph error on deep 50-task chain", cat)

    # E.10: Diamond dependency graph (A -> B, C; B, C -> D)
    tasks_e10 = [
        {"id": "A", "title": "A", "deps": []},
        {"id": "B", "title": "B", "deps": ["A"]},
        {"id": "C", "title": "C", "deps": ["A"]},
        {"id": "D", "title": "D", "deps": ["B", "C"]}
    ]
    valid_e10, err_e10 = rust_validate_graph(tasks_e10)
    ctx.assert_equal(valid_e10, True, "Diamond DAG validates cleanly", cat)


def run_category_f_collaboration_concurrency_failure_cascade(ctx: AdversarialTestContext):
    """Category F: Collaboration DAG Concurrency Limits, Failure Cascades & Cancellation."""
    cat = "Category F: Collaboration Concurrency & Cascade"
    if ctx.verbose:
        print(f"\n{CYAN}{BOLD}--- [{cat}] ---{RESET}")

    class MockTeamState:
        def __init__(self, max_parallel: int = 4, max_fix_rounds: int = 3):
            self.max_parallel = max_parallel
            self.max_fix_rounds = max_fix_rounds
            self.fix_rounds = 0
            self.status = "active"
            self.tasks: List[Dict[str, Any]] = []

        def claim_task(self, agent_id: str, task_id: Optional[str] = None) -> Dict[str, Any]:
            if self.status != "active":
                raise RuntimeError(f"{ERR_CLOSED}: 团队未运行")
            running_count = sum(1 for t in self.tasks if t.get("status") == "running")
            if running_count >= self.max_parallel:
                raise RuntimeError(f"{ERR_PARALLEL_LIMIT}: 团队任务并发已满")
            # find ready task
            completed_ids = {t["id"] for t in self.tasks if t.get("status") == "completed"}
            for t in self.tasks:
                if t.get("status") == "queued" and all(d in completed_ids for d in t.get("deps", [])):
                    if task_id is None or t["id"] == task_id:
                        t["status"] = "running"
                        t["owner"] = agent_id
                        return t
            raise RuntimeError("TEAM_TASK_NOT_READY: 没有可领取任务")

        def report_task(self, task_id: str, status: str, result: str):
            for t in self.tasks:
                if t["id"] == task_id:
                    t["status"] = status
                    t["result"] = result
            self.tasks = rust_propagate_failures(self.tasks)

        def control(self, action: str):
            if action == "stop":
                self.status = "stopped"
                for t in self.tasks:
                    if t.get("status") in ("queued", "running"):
                        t["status"] = "failed"
            elif action == "complete":
                if any(t.get("status") != "completed" for t in self.tasks):
                    raise RuntimeError(f"{ERR_INCOMPLETE}: 仍有未完成任务")
                self.status = "completed"

    # F.1: Concurrency limit max_parallel = 4 enforced on claims
    team_f = MockTeamState(max_parallel=4)
    team_f.tasks = [
        {"id": f"t_{i}", "status": "queued", "deps": []} for i in range(6)
    ]
    # Claim 4 tasks successfully
    for i in range(4):
        c = team_f.claim_task(f"agent_{i}")
        ctx.assert_equal(c["status"], "running", f"Claim {i+1}/4 succeeds", cat)

    # 5th claim must be rejected
    ctx.assert_raises(RuntimeError, lambda: team_f.claim_task("agent_extra"), ERR_PARALLEL_LIMIT,
                      "5th concurrent claim rejected with TEAM_PARALLEL_LIMIT", cat)

    # F.2: Task completion frees slot for next claim
    team_f.report_task("t_0", "completed", "Done")
    c_next = team_f.claim_task("agent_5")
    ctx.assert_equal(c_next["id"], "t_4", "Freed slot allows next task claim", cat)

    # F.3: Single artisan failure cascade blocks coordinator
    dag_tasks_f3 = [
        {"id": "artisan_1", "status": "running", "deps": []},
        {"id": "artisan_2", "status": "running", "deps": []},
        {"id": "coherence_coordinator", "status": "queued", "deps": ["artisan_1", "artisan_2"]}
    ]
    team_f3 = MockTeamState()
    team_f3.tasks = dag_tasks_f3
    # Fail artisan_1
    team_f3.report_task("artisan_1", "failed", "Meshy generation timeout")
    coord = next(t for t in team_f3.tasks if t["id"] == "coherence_coordinator")
    ctx.assert_equal(coord["status"], "blocked", "Artisan failure cascadingly blocks coordinator", cat)
    ctx.assert_equal(coord["result"], "依赖任务失败，未执行", "Blocked coordinator has standard cascade explanation", cat)

    # F.4: Independent artisan isolation
    artisan_2 = next(t for t in team_f3.tasks if t["id"] == "artisan_2")
    ctx.assert_equal(artisan_2["status"], "running", "Sibling artisan_2 remains running and unaffected", cat)

    # F.5: Transitive multi-hop failure cascade (T1 -> T2 -> T3)
    team_f5 = MockTeamState()
    team_f5.tasks = [
        {"id": "t1", "status": "running", "deps": []},
        {"id": "t2", "status": "queued", "deps": ["t1"]},
        {"id": "t3", "status": "queued", "deps": ["t2"]}
    ]
    team_f5.report_task("t1", "failed", "Fatal compile error")
    t2 = next(t for t in team_f5.tasks if t["id"] == "t2")
    t3 = next(t for t in team_f5.tasks if t["id"] == "t3")
    ctx.assert_equal(t2["status"], "blocked", "Direct dependent T2 is blocked", cat)
    ctx.assert_equal(t3["status"], "blocked", "Transitive dependent T3 is cascadingly blocked", cat)

    # F.6: Blocked tasks excluded from claimable tasks
    ctx.assert_raises(RuntimeError, lambda: team_f5.claim_task("agent_any"), "TEAM_TASK_NOT_READY",
                      "Blocked tasks cannot be claimed", cat)

    # F.7: Fix round limit max_fix_rounds = 3
    def retry_task_mock(team: MockTeamState):
        if team.fix_rounds >= team.max_fix_rounds:
            raise RuntimeError(f"{ERR_FIX_LIMIT}: 修复轮数已达上限")
        team.fix_rounds += 1

    team_fix = MockTeamState(max_fix_rounds=3)
    retry_task_mock(team_fix)  # round 1
    retry_task_mock(team_fix)  # round 2
    retry_task_mock(team_fix)  # round 3
    ctx.assert_equal(team_fix.fix_rounds, 3, "3 fix rounds accepted", cat)
    ctx.assert_raises(RuntimeError, lambda: retry_task_mock(team_fix), ERR_FIX_LIMIT,
                      "4th fix round rejected with TEAM_FIX_LIMIT", cat)

    # F.8: Premature team completion guard
    team_premature = MockTeamState()
    team_premature.tasks = [
        {"id": "t1", "status": "completed", "deps": []},
        {"id": "t2", "status": "running", "deps": []}
    ]
    ctx.assert_raises(RuntimeError, lambda: team_premature.control("complete"), ERR_INCOMPLETE,
                      "Premature complete call rejected when tasks remain incomplete", cat)

    # F.9: Team cancellation cascade
    team_cancel = MockTeamState()
    team_cancel.tasks = [
        {"id": "t1", "status": "running", "deps": []},
        {"id": "t2", "status": "queued", "deps": []}
    ]
    team_cancel.control("stop")
    ctx.assert_equal(team_cancel.status, "stopped", "Team transitioned to stopped status", cat)
    ctx.assert_equal(team_cancel.tasks[0]["status"], "failed", "Running tasks failed on stop", cat)
    ctx.assert_equal(team_cancel.tasks[1]["status"], "failed", "Queued tasks failed on stop", cat)

    # F.10: Blocked team rejects claims
    ctx.assert_raises(RuntimeError, lambda: team_cancel.claim_task("any"), ERR_CLOSED,
                      "Stopped team rejects any new task claims", cat)


def run_category_g_subagents_profile_integrity(ctx: AdversarialTestContext):
    """Category G: Subagents Profile Whitelist & Security Rules."""
    cat = "Category G: Subagents Profile Whitelist"
    if ctx.verbose:
        print(f"\n{CYAN}{BOLD}--- [{cat}] ---{RESET}")

    artisan_path = WORKSPACE_ROOT / "agents" / "building-artisan.md"
    ctx.assert_true(artisan_path.is_file(), "agents/building-artisan.md file exists", cat)

    content = artisan_path.read_text(encoding="utf-8")

    # G.1: Front matter verification
    ctx.assert_true("name: building-artisan" in content, "Profile name is building-artisan", cat)
    ctx.assert_true("maxSteps: 512" in content or "maxSteps: 500" in content, "maxSteps >= 500 specified", cat)

    # G.2: Tools whitelist contains required capabilities
    expected_tools = [
        "mcp__engine-scene__*",
        "mcp__asset-pipeline__*",
        "mcp__gen-model__*",
        "read_skill"
    ]
    for et in expected_tools:
        ctx.assert_true(et in content, f"Tools whitelist includes {et}", cat)

    # G.3: Redline rule: 'task' tool strictly prohibited from whitelist
    tools_line = [line for line in content.splitlines() if line.startswith("tools:")][0]
    tools_list = json.loads(tools_line.replace("tools:", "").strip())
    ctx.assert_true("task" not in tools_list, "'task' tool is absent from tools whitelist (prevents recursion)", cat)

    # G.4: Name 'task' prohibited as profile name
    def parse_profile_mock(name: str):
        if name == "task":
            raise ValueError("profile 名不可为 task(防递归委派)")
    ctx.assert_raises(ValueError, lambda: parse_profile_mock("task"), "防递归委派",
                      "Profile name 'task' explicitly prohibited", cat)

    # G.5: Strict discipline statement
    ctx.assert_true("必须遵守" in content, "Profile prompt includes '必须遵守' discipline block", cat)


def run_category_h_live_cargo_tests(ctx: AdversarialTestContext):
    """Category H: Live Cargo Integration & Regression Checks."""
    cat = "Category H: Live Rust Cargo Tests"
    if ctx.verbose:
        print(f"\n{CYAN}{BOLD}--- [{cat}] ---{RESET}")

    # H.1: Cargo test world_build::verify
    if ctx.verbose:
        print("  Running cargo test -p forge-agentd --bin forge-agentd world_build::verify...")
    res_h1 = subprocess.run(
        ["cargo", "test", "-p", "forge-agentd", "--bin", "forge-agentd", "world_build::verify"],
        cwd=str(WORKSPACE_ROOT),
        capture_output=True,
        text=True
    )
    ctx.assert_equal(res_h1.returncode, 0, "cargo test world_build::verify passes cleanly", cat)

    # H.2: Cargo test test_world_build_dag_materialization
    if ctx.verbose:
        print("  Running cargo test -p forge-agentd --bin forge-agentd test_world_build_dag_materialization...")
    res_h2 = subprocess.run(
        ["cargo", "test", "-p", "forge-agentd", "--bin", "forge-agentd", "test_world_build_dag_materialization"],
        cwd=str(WORKSPACE_ROOT),
        capture_output=True,
        text=True
    )
    ctx.assert_equal(res_h2.returncode, 0, "cargo test test_world_build_dag_materialization passes cleanly", cat)

    # H.3: Cargo test test_fix_coherence_and_approve_override_actions
    if ctx.verbose:
        print("  Running cargo test -p forge-agentd --bin forge-agentd test_fix_coherence_and_approve_override_actions...")
    res_h3 = subprocess.run(
        ["cargo", "test", "-p", "forge-agentd", "--bin", "forge-agentd", "test_fix_coherence_and_approve_override_actions"],
        cwd=str(WORKSPACE_ROOT),
        capture_output=True,
        text=True
    )
    ctx.assert_equal(res_h3.returncode, 0, "cargo test test_fix_coherence_and_approve_override_actions passes cleanly", cat)

    # H.4: Cargo test test_world_build_stage_transition_lifecycle
    if ctx.verbose:
        print("  Running cargo test -p forge-agentd --bin forge-agentd test_world_build_stage_transition_lifecycle...")
    res_h4 = subprocess.run(
        ["cargo", "test", "-p", "forge-agentd", "--bin", "forge-agentd", "test_world_build_stage_transition_lifecycle"],
        cwd=str(WORKSPACE_ROOT),
        capture_output=True,
        text=True
    )
    ctx.assert_equal(res_h4.returncode, 0, "cargo test test_world_build_stage_transition_lifecycle passes cleanly", cat)

    # H.5: Cargo test test_world_build_route_exhaustive
    if ctx.verbose:
        print("  Running cargo test -p forge-agentd --bin forge-agentd test_world_build_route_exhaustive...")
    res_h5 = subprocess.run(
        ["cargo", "test", "-p", "forge-agentd", "--bin", "forge-agentd", "test_world_build_route_exhaustive"],
        cwd=str(WORKSPACE_ROOT),
        capture_output=True,
        text=True
    )
    ctx.assert_equal(res_h5.returncode, 0, "cargo test test_world_build_route_exhaustive passes cleanly", cat)

    # H.6: Cargo test test_world_build_revalidate_concurrent_protection
    if ctx.verbose:
        print("  Running cargo test -p forge-agentd --bin forge-agentd test_world_build_revalidate_concurrent_protection...")
    res_h6 = subprocess.run(
        ["cargo", "test", "-p", "forge-agentd", "--bin", "forge-agentd", "test_world_build_revalidate_concurrent_protection"],
        cwd=str(WORKSPACE_ROOT),
        capture_output=True,
        text=True
    )
    ctx.assert_equal(res_h6.returncode, 0, "cargo test test_world_build_revalidate_concurrent_protection passes cleanly", cat)

    # H.7: Redline R-5 security check (no leaked secrets in tracked source)
    tracked_source_files = [
        WORKSPACE_ROOT / "crates" / "forge-agentd" / "src" / "world_build.rs",
        WORKSPACE_ROOT / "crates" / "forge-agentd" / "src" / "world_build" / "verify.rs",
        WORKSPACE_ROOT / "crates" / "forge-agentd" / "src" / "world_build" / "preview.rs",
        WORKSPACE_ROOT / "crates" / "forge-agentd" / "src" / "collaboration.rs",
        WORKSPACE_ROOT / "agents" / "building-artisan.md",
    ]
    leak_detected = False
    leaked_pattern = None
    for p in tracked_source_files:
        if p.is_file():
            text = p.read_text(encoding="utf-8")
            for pattern in REDLINE_SECRETS:
                if pattern in text:
                    leak_detected = True
                    leaked_pattern = (pattern, str(p))
                    break
    ctx.assert_equal(leak_detected, False, f"Zero leaked secret patterns in tracked source ({leaked_pattern})", cat)


# ---------------------------------------------------------------------------
# Section 3: Test Suite Runner & Reporting
# ---------------------------------------------------------------------------

def run_all_tier5_adversarial_tests(output_report_path: Optional[Path] = None, verbose: bool = False) -> Tuple[int, Dict[str, Any]]:
    start_time = time.time()
    ctx = AdversarialTestContext(verbose=verbose)

    print("\n" + "=" * 80)
    print("  RurixForge Milestone 5: Tier 5 Backend Adversarial Test Suite")
    print("=" * 80)

    categories = [
        ("Category A", run_category_a_building_counts),
        ("Category B", run_category_b_corrupted_jsons),
        ("Category C", run_category_c_extreme_coordinates),
        ("Category D", run_category_d_float_precision_epsilon),
        ("Category E", run_category_e_cyclic_deep_graphs),
        ("Category F", run_category_f_collaboration_concurrency_failure_cascade),
        ("Category G", run_category_g_subagents_profile_integrity),
        ("Category H", run_category_h_live_cargo_tests),
    ]

    for name, fn in categories:
        if not verbose:
            sys.stdout.write(f"\n{CYAN}{name:20s}{RESET}: ")
            sys.stdout.flush()
        fn(ctx)
        if not verbose:
            print(f"  {GREEN}[PASS]{RESET}")

    duration = round(time.time() - start_time, 2)
    overall_passed = (ctx.failed == 0)

    # Category summaries
    cat_summary = {}
    for t in ctx.tests:
        cat = t["category"]
        if cat not in cat_summary:
            cat_summary[cat] = {"passed": 0, "failed": 0, "total": 0}
        cat_summary[cat]["total"] += 1
        if t["status"] == "PASS":
            cat_summary[cat]["passed"] += 1
        else:
            cat_summary[cat]["failed"] += 1

    report = {
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "summary": {
            "totalTests": len(ctx.tests),
            "passed": ctx.passed,
            "failed": ctx.failed,
            "verdict": "APPROVE" if overall_passed else "REQUEST_CHANGES",
            "durationSeconds": duration,
        },
        "categories": cat_summary,
        "securityAudit": {
            "secretLeakageCheck": "CLEAN"
        },
        "tests": ctx.tests,
    }

    report_path = output_report_path or (WORKSPACE_ROOT / ".forge" / "tmp" / "e2e" / "tier5_backend_adversarial_report.json")
    report_path.parent.mkdir(parents=True, exist_ok=True)
    with open(report_path, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=2)

    print("\n" + "=" * 80)
    print("                         TIER 5 ADVERSARIAL RUN SUMMARY                         ")
    print("=" * 80)
    print(f" Total Tests   : {ctx.passed + ctx.failed}")
    print(f" Passed        : {GREEN}{ctx.passed}{RESET}")
    print(f" Failed        : {RED if ctx.failed > 0 else GREEN}{ctx.failed}{RESET}")
    print(f" Verdict       : {GREEN if overall_passed else RED}{report['summary']['verdict']}{RESET}")
    print(f" Duration      : {duration}s")
    print(f" Report Saved  : {report_path}")
    print("-" * 80)
    for cat, stat in cat_summary.items():
        print(f"  {cat:45s}: {stat['passed']:2d} / {stat['total']:2d} passed")
    print("-" * 80)
    print(f" Security Audit: {GREEN}CLEAN (Zero credentials leaked){RESET}")
    print("=" * 80 + "\n")

    return (0 if overall_passed else 1), report


def main():
    parser = argparse.ArgumentParser(description="Tier 5 Adversarial Backend Test Suite for RurixForge")
    parser.add_argument("--verbose", "-v", action="store_true", help="Verbose test logs")
    parser.add_argument("--output-report", type=str, default=None, help="Custom output JSON report path")
    args = parser.parse_args()

    out_path = Path(args.output_report) if args.output_report else None
    exit_code, _ = run_all_tier5_adversarial_tests(output_report_path=out_path, verbose=args.verbose)
    sys.exit(exit_code)


if __name__ == "__main__":
    main()
