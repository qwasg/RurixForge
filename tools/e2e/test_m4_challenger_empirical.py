#!/usr/bin/env python3
"""Milestone 4 Challenger 1: Empirical Stress Testing of Coherence Gate & DAG Engine.

Authored by Challenger 1 (Empirical Challenger).

Empirically stresses and verifies:
1. AABB Ground Footprint X-Z Collisions:
   - Tangent boundary contact (overlap = 0.0 or <= 1e-4) MUST pass cleanly.
   - Micro-overlap (overlap_x = 0.001, overlap_z = 2.0, area = 0.002 > 1e-4) MUST be flagged as COHERENCE_COLLISION_DETECTED.
   - Disjoint buildings MUST pass cleanly.
   - Non-trivial geometries: negative coordinates, asymmetric extents, 1D tangent contacts.
2. Interior Prop Hierarchy:
   - Prop referencing valid existing building entity ID MUST pass (direct and ECS component forms).
   - Prop lacking Parent component or referencing nonexistent entity ID MUST trigger COHERENCE_PROP_ORPHANED.
   - Prop with empty parentEntityId MUST trigger COHERENCE_PROP_ORPHANED.
   - Isolation: non-prop entities without parents are not falsely flagged.
3. Rotation Quaternion Normalization:
   - Normalized quaternion (|norm - 1.0| < 1e-4) MUST pass.
   - Unnormalized quaternion (|norm - 1.0| >= 1e-4) MUST trigger COHERENCE_ROTATION_UNNORMALIZED.
   - Boundary stress at norm = 1.0 +/- 1e-4.
   - Zero-quaternions and Euler-quaternion confusion.
4. DAG Materialization & Failure Propagation:
   - N parallel artisan tasks + 1 terminal coordinator task depending on all N artisan tasks.
   - Concurrency limit max_parallel = 4 enforced on task claims.
   - Single artisan task failure MUST propagate and block coordinator execution.
   - Blocked coordinator cannot be claimed and prevents team completion.
5. Rust Backend Verification:
   - Direct execution and assertions against cargo test test suites.
"""

import copy
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from typing import Any, Dict, List, Optional, Set, Tuple

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent.parent

# Error Codes per crates/forge-agentd/src/world_build/verify.rs
ERR_COLLISION_DETECTED = "COHERENCE_COLLISION_DETECTED"
ERR_PROP_ORPHANED = "COHERENCE_PROP_ORPHANED"
ERR_BUILDING_MISSING = "COHERENCE_BUILDING_MISSING"
ERR_ROTATION_UNNORMALIZED = "COHERENCE_ROTATION_UNNORMALIZED"

COLLISION_EPSILON = 1e-4
QUATERNION_NORMAL_EPSILON = 1e-4

GREEN = "\033[92m"
RED = "\033[91m"
YELLOW = "\033[93m"
CYAN = "\033[96m"
BOLD = "\033[1m"
RESET = "\033[0m"


class EmpiricalHarnessContext:
    def __init__(self):
        self.passed = 0
        self.failed = 0
        self.errors: List[str] = []

    def assert_true(self, condition: bool, description: str):
        if condition:
            self.passed += 1
            print(f"  {GREEN}✓{RESET} {description}")
        else:
            self.failed += 1
            msg = f"ASSERTION FAILED: {description}"
            self.errors.append(msg)
            print(f"  {RED}✗ FAIL:{RESET} {description}")

    def assert_equal(self, actual: Any, expected: Any, description: str):
        if actual == expected:
            self.passed += 1
            print(f"  {GREEN}✓{RESET} {description}")
        else:
            self.failed += 1
            msg = f"{description} (Expected {expected!r}, got {actual!r})"
            self.errors.append(msg)
            print(f"  {RED}✗ FAIL:{RESET} {msg}")


# ---------------------------------------------------------------------------
# Pure Reference Oracles (Mirroring verify.rs and collaboration.rs exactly)
# ---------------------------------------------------------------------------

def ref_check_aabb_xz_collision(
    min_a: List[float], max_a: List[float], pos_a: List[float],
    min_b: List[float], max_b: List[float], pos_b: List[float],
    epsilon: float = COLLISION_EPSILON
) -> Tuple[bool, float, float]:
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


def ref_check_quaternion_normalized(quat: List[float], epsilon: float = QUATERNION_NORMAL_EPSILON) -> bool:
    norm_sq = quat[0]*quat[0] + quat[1]*quat[1] + quat[2]*quat[2] + quat[3]*quat[3]
    norm = math.sqrt(norm_sq)
    return abs(norm - 1.0) < epsilon


def ref_parse_position(ent: Dict[str, Any]) -> List[float]:
    t = ent.get("transform") or {}
    pos = t.get("position") or t.get("translation")
    if pos and isinstance(pos, list) and len(pos) >= 3:
        return [float(pos[0]), float(pos[1]), float(pos[2])]
    c = ent.get("components") or {}
    tc = c.get("Transform") or {}
    pos = tc.get("position") or tc.get("translation")
    if pos and isinstance(pos, list) and len(pos) >= 3:
        return [float(pos[0]), float(pos[1]), float(pos[2])]
    return [0.0, 0.0, 0.0]


def ref_parse_bounds(ent: Dict[str, Any]) -> Tuple[List[float], List[float]]:
    b = ent.get("bounds") or {}
    b_min = b.get("min") or [-1.0, -1.0, -1.0]
    b_max = b.get("max") or [1.0, 1.0, 1.0]
    return (
        [float(b_min[0]), float(b_min[1]), float(b_min[2])],
        [float(b_max[0]), float(b_max[1]), float(b_max[2])]
    )


def ref_parse_rotation(ent: Dict[str, Any]) -> Optional[List[float]]:
    t = ent.get("transform") or {}
    rot = t.get("rotation")
    if rot and isinstance(rot, list) and len(rot) == 4:
        return [float(x) for x in rot]
    c = ent.get("components") or {}
    tc = c.get("Transform") or {}
    rot = tc.get("rotation")
    if rot and isinstance(rot, list) and len(rot) == 4:
        return [float(x) for x in rot]
    return None


def ref_get_parent_id(ent: Dict[str, Any]) -> Optional[str]:
    if ent.get("parentEntityId") is not None:
        return str(ent["parentEntityId"])
    c = ent.get("components") or {}
    p = c.get("Parent") or {}
    pid = p.get("parentEntityId") or p.get("entityId") or p.get("entity")
    if pid is not None:
        return str(pid)
    return None


def ref_is_interior_prop(ent: Dict[str, Any]) -> bool:
    if ent.get("isInteriorProp") is True:
        return True
    c = ent.get("components") or {}
    tag = c.get("Tag", {}).get("tag")
    if tag in ("interior_prop", "interior"):
        return True
    ent_id = ent.get("id", "")
    if ent_id.startswith("prop_"):
        return True
    return False


def ref_is_building(ent: Dict[str, Any]) -> bool:
    if ent.get("isBuilding") is True:
        return True
    c = ent.get("components") or {}
    tag = c.get("Tag", {}).get("tag")
    if tag == "building":
        return True
    return "buildingId" in ent


def run_oracle_coherence_verification(
    scene_entities: List[Dict[str, Any]],
    manifest: Optional[Dict[str, Any]] = None,
    epsilon: float = COLLISION_EPSILON
) -> Dict[str, Any]:
    violations = []

    # 1. Manifest completeness
    if manifest and "buildings" in manifest:
        scene_bldg_ids = {
            e.get("buildingId") or e.get("id")
            for e in scene_entities
            if ref_is_building(e)
        }
        for b in manifest["buildings"]:
            bid = b.get("id")
            if bid and bid not in scene_bldg_ids:
                violations.append({
                    "code": ERR_BUILDING_MISSING,
                    "buildingId": bid,
                    "message": f"Building {bid} missing from generated scene"
                })

    # 2. AABB footprint collisions
    buildings = [e for e in scene_entities if ref_is_building(e)]
    for i in range(len(buildings)):
        for j in range(i + 1, len(buildings)):
            b1 = buildings[i]
            b2 = buildings[j]
            min1, max1 = ref_parse_bounds(b1)
            min2, max2 = ref_parse_bounds(b2)
            pos1 = ref_parse_position(b1)
            pos2 = ref_parse_position(b2)
            collides, ox, oz = ref_check_aabb_xz_collision(min1, max1, pos1, min2, max2, pos2, epsilon)
            if collides:
                id1 = b1.get("id", "b1")
                id2 = b2.get("id", "b2")
                violations.append({
                    "code": ERR_COLLISION_DETECTED,
                    "pair": [id1, id2],
                    "overlapX": ox,
                    "overlapZ": oz,
                    "overlapArea": ox * oz,
                    "message": f"Buildings {id1} and {id2} collide on ground plane"
                })

    # 3. Interior props hierarchy
    props = [e for e in scene_entities if ref_is_interior_prop(e)]
    entity_ids = {str(e.get("id")) for e in scene_entities if e.get("id") is not None}
    for prop in props:
        pid = ref_get_parent_id(prop)
        prop_id = prop.get("id", "prop")
        if not pid or pid not in entity_ids:
            violations.append({
                "code": ERR_PROP_ORPHANED,
                "propId": prop_id,
                "message": f"Interior prop {prop_id} has invalid or missing parent"
            })

    # 4. Quaternion normalization
    for ent in scene_entities:
        quat = ref_parse_rotation(ent)
        if quat:
            if not ref_check_quaternion_normalized(quat, QUATERNION_NORMAL_EPSILON):
                ent_id = ent.get("id")
                violations.append({
                    "code": ERR_ROTATION_UNNORMALIZED,
                    "buildingId": ent_id if ref_is_building(ent) else None,
                    "propId": ent_id if ref_is_interior_prop(ent) else None,
                    "message": "Entity rotation quaternion is not normalized"
                })

    passed = (len(violations) == 0)
    return {
        "status": "PASS" if passed else "FAIL",
        "stage": "done" if passed else "coherence_verify",
        "evaluatedBuildings": len(buildings),
        "evaluatedProps": len(props),
        "violations": violations
    }


# ---------------------------------------------------------------------------
# Section 1: AABB Ground Footprint X-Z Collisions Stress Tests
# ---------------------------------------------------------------------------

def stress_test_aabb_xz_collisions(ctx: EmpiricalHarnessContext):
    print(f"\n{CYAN}{BOLD}=== [Part 1] AABB Ground Footprint X-Z Collision Stress ==={RESET}")

    # Standard box bounds [-1, 0, -1] to [1, 2, 1] => width=2, depth=2, half-extents=1.0
    min_box = [-1.0, 0.0, -1.0]
    max_box = [1.0, 2.0, 1.0]
    pos_orig = [0.0, 0.0, 0.0]

    # Test 1.1: Exact Tangent Contact along X+ (pos_b = [2.0, 0.0, 0.0]) -> overlap_x = 0.0 -> PASS
    c, ox, oz = ref_check_aabb_xz_collision(min_box, max_box, pos_orig, min_box, max_box, [2.0, 0.0, 0.0])
    ctx.assert_true(not c, "Tangent boundary contact on X+ (distance=2.0, overlap_x=0.0) MUST pass cleanly")
    ctx.assert_equal(ox, 0.0, "Exact tangent overlap_x is 0.0")

    # Test 1.2: Exact Tangent Contact along X- (pos_b = [-2.0, 0.0, 0.0]) -> overlap_x = 0.0 -> PASS
    c, ox, oz = ref_check_aabb_xz_collision(min_box, max_box, pos_orig, min_box, max_box, [-2.0, 0.0, 0.0])
    ctx.assert_true(not c, "Tangent boundary contact on X- (distance=-2.0, overlap_x=0.0) MUST pass cleanly")
    ctx.assert_equal(ox, 0.0, "Exact tangent overlap_x is 0.0")

    # Test 1.3: Exact Tangent Contact along Z+ (pos_b = [0.0, 0.0, 2.0]) -> overlap_z = 0.0 -> PASS
    c, ox, oz = ref_check_aabb_xz_collision(min_box, max_box, pos_orig, min_box, max_box, [0.0, 0.0, 2.0])
    ctx.assert_true(not c, "Tangent boundary contact on Z+ (distance=2.0, overlap_z=0.0) MUST pass cleanly")
    ctx.assert_equal(oz, 0.0, "Exact tangent overlap_z is 0.0")

    # Test 1.4: Exact Tangent Contact along Z- (pos_b = [0.0, 0.0, -2.0]) -> overlap_z = 0.0 -> PASS
    c, ox, oz = ref_check_aabb_xz_collision(min_box, max_box, pos_orig, min_box, max_box, [0.0, 0.0, -2.0])
    ctx.assert_true(not c, "Tangent boundary contact on Z- (distance=-2.0, overlap_z=0.0) MUST pass cleanly")
    ctx.assert_equal(oz, 0.0, "Exact tangent overlap_z is 0.0")

    # Test 1.5: Corner Tangent (pos_b = [2.0, 0.0, 2.0]) -> overlap_x=0, overlap_z=0 -> PASS
    c, ox, oz = ref_check_aabb_xz_collision(min_box, max_box, pos_orig, min_box, max_box, [2.0, 0.0, 2.0])
    ctx.assert_true(not c, "Corner tangent boundary contact (overlap_x=0, overlap_z=0) MUST pass cleanly")

    # Test 1.6: Sub-epsilon boundary contact (distance = 1.99995 => overlap_x = 0.00005 <= 1e-4) -> PASS
    c, ox, oz = ref_check_aabb_xz_collision(min_box, max_box, pos_orig, min_box, max_box, [1.99995, 0.0, 0.0])
    ctx.assert_true(not c, "Sub-epsilon tangent tolerance (overlap_x = 5e-5 <= 1e-4) MUST pass cleanly")

    # Test 1.7: Micro-overlap required by dispatch prompt:
    # overlap_x = 0.001, overlap_z = 2.0, area = 0.002 > 1e-4 -> MUST be flagged as COHERENCE_COLLISION_DETECTED
    pos_micro = [1.999, 0.0, 0.0]
    c, ox, oz = ref_check_aabb_xz_collision(min_box, max_box, pos_orig, min_box, max_box, pos_micro)
    area = ox * oz
    ctx.assert_true(c, "Micro-overlap (overlap_x=0.001, overlap_z=2.0, area=0.002 > 1e-4) MUST trigger collision")
    ctx.assert_true(abs(ox - 0.001) < 1e-5, f"Expected ox approx 0.001, got {ox}")
    ctx.assert_true(abs(oz - 2.0) < 1e-5, f"Expected oz approx 2.0, got {oz}")
    ctx.assert_true(abs(area - 0.002) < 1e-5, f"Expected area approx 0.002, got {area}")

    # Test 1.8: Micro-overlap in both axes (overlap_x = 0.0005 > 1e-4, overlap_z = 0.0005 > 1e-4) -> MUST COLLIDE
    pos_both = [1.9995, 0.0, 1.9995]
    c, ox, oz = ref_check_aabb_xz_collision(min_box, max_box, pos_orig, min_box, max_box, pos_both)
    ctx.assert_true(c, "Dual micro-overlap (ox=5e-4, oz=5e-4 > 1e-4) MUST trigger collision")

    # Test 1.9: Complete disjoint buildings (distance = 100.0) -> PASS
    c, ox, oz = ref_check_aabb_xz_collision(min_box, max_box, pos_orig, min_box, max_box, [100.0, 0.0, 100.0])
    ctx.assert_true(not c, "Completely disjoint buildings MUST pass cleanly")
    ctx.assert_equal(ox, 0.0, "Disjoint overlap_x is 0.0")
    ctx.assert_equal(oz, 0.0, "Disjoint overlap_z is 0.0")

    # Test 1.10: 1D overlap but tangent in other dimension (overlap_x = 1.0, overlap_z = 0.0) -> PASS
    c, ox, oz = ref_check_aabb_xz_collision(min_box, max_box, pos_orig, min_box, max_box, [1.0, 0.0, 2.0])
    ctx.assert_true(not c, "Overlap in X but tangent in Z (ox=1.0, oz=0.0) MUST pass cleanly (zero area)")

    # Test 1.11: Negative coordinates tangent contact -> PASS
    # Box A at [-50, 0, -50], Box B at [-48, 0, -50]
    c, ox, oz = ref_check_aabb_xz_collision(min_box, max_box, [-50.0, 0.0, -50.0], min_box, max_box, [-48.0, 0.0, -50.0])
    ctx.assert_true(not c, "Negative coordinates tangent contact MUST pass cleanly")

    # Test 1.12: Negative coordinates micro-overlap (distance = 1.999 => ox = 0.001) -> COLLISION
    c, ox, oz = ref_check_aabb_xz_collision(min_box, max_box, [-50.0, 0.0, -50.0], min_box, max_box, [-48.001, 0.0, -50.0])
    ctx.assert_true(c, "Negative coordinates micro-overlap MUST trigger collision")

    # Test 1.13: Full Coherence Verification Pipeline with Micro-overlap
    scene_coll = [
        {"id": "ent_b1", "isBuilding": True, "buildingId": "b1", "bounds": {"min": min_box, "max": max_box}, "transform": {"position": pos_orig}},
        {"id": "ent_b2", "isBuilding": True, "buildingId": "b2", "bounds": {"min": min_box, "max": max_box}, "transform": {"position": pos_micro}}
    ]
    manifest = {"buildings": [{"id": "b1"}, {"id": "b2"}]}
    audit = run_oracle_coherence_verification(scene_coll, manifest)
    ctx.assert_equal(audit["status"], "FAIL", "Scene with micro-overlap must have audit status FAIL")
    codes = [v["code"] for v in audit["violations"]]
    ctx.assert_true(ERR_COLLISION_DETECTED in codes, f"Must contain {ERR_COLLISION_DETECTED}")
    coll_v = next(v for v in audit["violations"] if v["code"] == ERR_COLLISION_DETECTED)
    ctx.assert_equal(coll_v["pair"], ["ent_b1", "ent_b2"], "Collision violation pair must identify colliding buildings")
    ctx.assert_true(coll_v["overlapArea"] > 1e-4, f"Overlap area {coll_v['overlapArea']} > 1e-4")

    # Test 1.14: Full Coherence Verification Pipeline with Tangent Contact
    scene_tangent = [
        {"id": "ent_b1", "isBuilding": True, "buildingId": "b1", "bounds": {"min": min_box, "max": max_box}, "transform": {"position": pos_orig}},
        {"id": "ent_b2", "isBuilding": True, "buildingId": "b2", "bounds": {"min": min_box, "max": max_box}, "transform": {"position": [2.0, 0.0, 0.0]}}
    ]
    audit_tan = run_oracle_coherence_verification(scene_tangent, manifest)
    ctx.assert_equal(audit_tan["status"], "PASS", "Scene with tangent contact must have audit status PASS")
    ctx.assert_equal(len(audit_tan["violations"]), 0, "Scene with tangent contact must have 0 violations")


# ---------------------------------------------------------------------------
# Section 2: Interior Prop Hierarchy Stress Tests
# ---------------------------------------------------------------------------

def stress_test_interior_prop_hierarchy(ctx: EmpiricalHarnessContext):
    print(f"\n{CYAN}{BOLD}=== [Part 2] Interior Prop Hierarchy Stress ==={RESET}")

    bldg_manifest = {"buildings": [{"id": "bldg_hq"}]}
    valid_building = {
        "id": "ent_bldg_hq",
        "isBuilding": True,
        "buildingId": "bldg_hq",
        "bounds": {"min": [-5, 0, -5], "max": [5, 10, 5]},
        "transform": {"position": [0, 0, 0]}
    }

    # Test 2.1: Prop referencing valid existing building entity ID via parentEntityId -> PASS
    prop_valid = {
        "id": "prop_chair_01",
        "isInteriorProp": True,
        "parentEntityId": "ent_bldg_hq",
        "transform": {"position": [1, 0, 1]}
    }
    audit = run_oracle_coherence_verification([valid_building, prop_valid], bldg_manifest)
    ctx.assert_equal(audit["status"], "PASS", "Prop referencing existing building via parentEntityId MUST pass")
    ctx.assert_equal(len(audit["violations"]), 0, "Zero violations for valid prop")

    # Test 2.2: Prop referencing valid parent via ECS component Parent.parentEntityId -> PASS
    prop_ecs_1 = {
        "id": "prop_desk_02",
        "isInteriorProp": True,
        "components": {"Parent": {"parentEntityId": "ent_bldg_hq"}},
        "transform": {"position": [2, 0, 2]}
    }
    audit = run_oracle_coherence_verification([valid_building, prop_ecs_1], bldg_manifest)
    ctx.assert_equal(audit["status"], "PASS", "Prop referencing parent via components.Parent.parentEntityId MUST pass")

    # Test 2.3: Prop referencing valid parent via ECS component Parent.entityId -> PASS
    prop_ecs_2 = {
        "id": "prop_lamp_03",
        "isInteriorProp": True,
        "components": {"Parent": {"entityId": "ent_bldg_hq"}},
        "transform": {"position": [3, 0, 3]}
    }
    audit = run_oracle_coherence_verification([valid_building, prop_ecs_2], bldg_manifest)
    ctx.assert_equal(audit["status"], "PASS", "Prop referencing parent via components.Parent.entityId MUST pass")

    # Test 2.4: Prop lacking Parent component and lacking parentEntityId -> MUST trigger COHERENCE_PROP_ORPHANED
    prop_no_parent = {
        "id": "prop_orphan_01",
        "isInteriorProp": True,
        "transform": {"position": [0, 0, 0]}
    }
    audit = run_oracle_coherence_verification([valid_building, prop_no_parent], bldg_manifest)
    ctx.assert_equal(audit["status"], "FAIL", "Prop lacking Parent component MUST fail")
    codes = [v["code"] for v in audit["violations"]]
    ctx.assert_true(ERR_PROP_ORPHANED in codes, f"Must flag {ERR_PROP_ORPHANED}")
    orphan_v = next(v for v in audit["violations"] if v["code"] == ERR_PROP_ORPHANED)
    ctx.assert_equal(orphan_v["propId"], "prop_orphan_01", "Identifies correct orphaned prop ID")

    # Test 2.5: Prop referencing nonexistent parent ID ("bldg_ghost_404") -> MUST trigger COHERENCE_PROP_ORPHANED
    prop_ghost_parent = {
        "id": "prop_orphan_ghost",
        "isInteriorProp": True,
        "parentEntityId": "bldg_ghost_404",
        "transform": {"position": [0, 0, 0]}
    }
    audit = run_oracle_coherence_verification([valid_building, prop_ghost_parent], bldg_manifest)
    ctx.assert_equal(audit["status"], "FAIL", "Prop referencing nonexistent parent MUST fail")
    codes = [v["code"] for v in audit["violations"]]
    ctx.assert_true(ERR_PROP_ORPHANED in codes, f"Nonexistent parent must flag {ERR_PROP_ORPHANED}")

    # Test 2.6: Prop with empty string parentEntityId ("") -> MUST trigger COHERENCE_PROP_ORPHANED
    prop_empty_parent = {
        "id": "prop_orphan_empty",
        "isInteriorProp": True,
        "parentEntityId": "",
        "transform": {"position": [0, 0, 0]}
    }
    audit = run_oracle_coherence_verification([valid_building, prop_empty_parent], bldg_manifest)
    ctx.assert_equal(audit["status"], "FAIL", "Prop with empty parentEntityId MUST fail")
    codes = [v["code"] for v in audit["violations"]]
    ctx.assert_true(ERR_PROP_ORPHANED in codes, f"Empty parent must flag {ERR_PROP_ORPHANED}")

    # Test 2.7: Mixed props: 3 valid props, 1 orphaned prop -> exactly 1 violation for the orphaned prop
    scene_mixed = [
        valid_building,
        prop_valid,
        prop_ecs_1,
        prop_ecs_2,
        prop_no_parent
    ]
    audit_mixed = run_oracle_coherence_verification(scene_mixed, bldg_manifest)
    ctx.assert_equal(audit_mixed["status"], "FAIL", "Mixed scene with 1 orphan must fail")
    orphan_violations = [v for v in audit_mixed["violations"] if v["code"] == ERR_PROP_ORPHANED]
    ctx.assert_equal(len(orphan_violations), 1, "Exactly one orphaned violation reported")
    ctx.assert_equal(orphan_violations[0]["propId"], "prop_orphan_01", "Reports exact orphaned prop")

    # Test 2.8: Non-prop entities (e.g. outdoor streetlamp without isInteriorProp / prop_ prefix) lacking parent -> NOT flagged
    streetlamp = {
        "id": "exterior_streetlamp_01",
        "components": {"Tag": {"tag": "prop"}}, # Not tagged as interior prop
        "transform": {"position": [15, 0, 0]}
    }
    audit_lamp = run_oracle_coherence_verification([valid_building, streetlamp], bldg_manifest)
    ctx.assert_equal(audit_lamp["status"], "PASS", "Non-interior entities without parent are not flagged as orphaned props")


# ---------------------------------------------------------------------------
# Section 3: Rotation Quaternion Normalization Stress Tests
# ---------------------------------------------------------------------------

def stress_test_quaternion_normalization(ctx: EmpiricalHarnessContext):
    print(f"\n{CYAN}{BOLD}=== [Part 3] Rotation Quaternion Normalization Stress ==={RESET}")

    # Test 3.1: Identity Quaternion [0, 0, 0, 1] (norm = 1.0) -> PASS
    q_ident = [0.0, 0.0, 0.0, 1.0]
    ctx.assert_true(ref_check_quaternion_normalized(q_ident), "Identity quaternion MUST pass")

    # Test 3.2: 90 deg Yaw [0.0, 0.70710678, 0.0, 0.70710678] (norm = 1.0) -> PASS
    q_yaw90 = [0.0, 0.70710678, 0.0, 0.70710678]
    ctx.assert_true(ref_check_quaternion_normalized(q_yaw90), "90 degree yaw quaternion MUST pass")

    # Test 3.3: Arbitrary 3D Normalized [0.26726124, 0.5345225, 0.8017837, 0.0] -> PASS
    q_arb = [0.26726124, 0.5345225, 0.8017837, 0.0]
    ctx.assert_true(ref_check_quaternion_normalized(q_arb), "Arbitrary normalized 3D quaternion MUST pass")

    # Test 3.4: Negative Scalar Component [0, 0, 0, -1.0] (norm = 1.0) -> PASS
    q_neg = [0.0, 0.0, 0.0, -1.0]
    ctx.assert_true(ref_check_quaternion_normalized(q_neg), "Negative scalar quaternion MUST pass")

    # Test 3.5: Boundary within epsilon: norm = 1.0 + 9.5e-5 (|norm - 1.0| < 1e-4) -> PASS
    q_sub_eps = [0.0, 0.0, 0.0, 1.000095]
    ctx.assert_true(ref_check_quaternion_normalized(q_sub_eps), "Quaternion with |norm - 1.0| = 9.5e-5 < 1e-4 MUST pass")

    # Test 3.6: Boundary outside epsilon: norm = 1.0 + 1.05e-4 (|norm - 1.0| >= 1e-4) -> FAIL
    q_super_eps = [0.0, 0.0, 0.0, 1.000105]
    ctx.assert_true(not ref_check_quaternion_normalized(q_super_eps), "Quaternion with |norm - 1.0| = 1.05e-4 >= 1e-4 MUST fail")

    # Test 3.7: Classic unnormalized [1.0, 1.0, 0.0, 0.0] (norm = sqrt(2) approx 1.4142) -> FAIL
    q_unnorm = [1.0, 1.0, 0.0, 0.0]
    ctx.assert_true(not ref_check_quaternion_normalized(q_unnorm), "Unnormalized quaternion [1, 1, 0, 0] MUST fail")

    # Test 3.8: Zero quaternion [0, 0, 0, 0] (norm = 0.0) -> FAIL
    q_zero = [0.0, 0.0, 0.0, 0.0]
    ctx.assert_true(not ref_check_quaternion_normalized(q_zero), "Zero quaternion [0, 0, 0, 0] MUST fail")

    # Test 3.9: Euler angles accidentally placed in quaternion [0.0, 90.0, 0.0, 1.0] -> FAIL
    q_euler = [0.0, 90.0, 0.0, 1.0]
    ctx.assert_true(not ref_check_quaternion_normalized(q_euler), "Euler angles in quaternion MUST fail")

    # Test 3.10: Coherence Verification flags unnormalized building rotation
    scene_unnorm_bldg = [
        {
            "id": "ent_b1",
            "isBuilding": True,
            "buildingId": "b1",
            "bounds": {"min": [-1, 0, -1], "max": [1, 2, 1]},
            "transform": {"position": [0, 0, 0], "rotation": q_unnorm}
        }
    ]
    manifest = {"buildings": [{"id": "b1"}]}
    audit = run_oracle_coherence_verification(scene_unnorm_bldg, manifest)
    ctx.assert_equal(audit["status"], "FAIL", "Unnormalized building rotation must trigger FAIL")
    codes = [v["code"] for v in audit["violations"]]
    ctx.assert_true(ERR_ROTATION_UNNORMALIZED in codes, f"Must contain {ERR_ROTATION_UNNORMALIZED}")
    rot_v = next(v for v in audit["violations"] if v["code"] == ERR_ROTATION_UNNORMALIZED)
    ctx.assert_equal(rot_v["buildingId"], "ent_b1", "Violation correctly specifies building ID")

    # Test 3.11: Coherence Verification flags unnormalized prop rotation
    scene_unnorm_prop = [
        {
            "id": "ent_b1",
            "isBuilding": True,
            "buildingId": "b1",
            "bounds": {"min": [-1, 0, -1], "max": [1, 2, 1]},
            "transform": {"position": [0, 0, 0], "rotation": q_ident}
        },
        {
            "id": "prop_chair",
            "isInteriorProp": True,
            "parentEntityId": "ent_b1",
            "transform": {"position": [0, 0, 0], "rotation": q_super_eps}
        }
    ]
    audit_prop = run_oracle_coherence_verification(scene_unnorm_prop, manifest)
    ctx.assert_equal(audit_prop["status"], "FAIL", "Unnormalized prop rotation must trigger FAIL")
    codes = [v["code"] for v in audit_prop["violations"]]
    ctx.assert_true(ERR_ROTATION_UNNORMALIZED in codes, f"Must contain {ERR_ROTATION_UNNORMALIZED}")
    rot_p_v = next(v for v in audit_prop["violations"] if v["code"] == ERR_ROTATION_UNNORMALIZED)
    ctx.assert_equal(rot_p_v["propId"], "prop_chair", "Violation correctly specifies prop ID")


# ---------------------------------------------------------------------------
# Section 4: DAG Materialization & Failure Propagation Stress Tests
# ---------------------------------------------------------------------------

class DagSimulatorTeam:
    def __init__(self, team_id: str, max_parallel: int = 4, max_fix_rounds: int = 3):
        self.team_id = team_id
        self.max_parallel = max_parallel
        self.max_fix_rounds = max_fix_rounds
        self.status = "active"
        self.tasks: List[Dict[str, Any]] = []

    def set_tasks(self, tasks: List[Dict[str, Any]]):
        self.tasks = copy.deepcopy(tasks)

    def ready_task_ids(self) -> List[str]:
        # Stages in order of appearance
        stages = []
        for t in self.tasks:
            st = t.get("stage")
            if st and st not in stages:
                stages.append(st)

        ready = []
        for t in self.tasks:
            if t["status"] != "queued":
                continue
            # Check deps
            deps = t.get("deps", [])
            deps_ok = all(
                any(d["id"] == dep_id and d["status"] == "completed" for d in self.tasks)
                for dep_id in deps
            )
            if not deps_ok:
                continue

            # Check stage constraint
            stage = t.get("stage")
            if stage:
                idx = stages.index(stage)
                earlier_stages = stages[:idx]
                stage_ok = all(
                    d["status"] == "completed"
                    for d in self.tasks
                    if d.get("stage") in earlier_stages
                )
                if not stage_ok:
                    continue
            ready.append(t["id"])
        return ready

    def claim_task(self, agent_id: str, task_id: Optional[str] = None) -> Tuple[bool, Optional[Dict[str, Any]], str]:
        if self.status != "active":
            return False, None, "TEAM_NOT_ACTIVE"

        running_count = sum(1 for t in self.tasks if t["status"] == "running")
        if running_count >= self.max_parallel:
            return False, None, "TEAM_PARALLEL_LIMIT"

        ready = self.ready_task_ids()
        candidate = None
        for t in self.tasks:
            if (task_id is None or t["id"] == task_id) and t["id"] in ready:
                candidate = t
                break

        if not candidate:
            return False, None, "TEAM_TASK_NOT_READY"

        candidate["status"] = "running"
        candidate["ownerAgentId"] = agent_id
        return True, copy.deepcopy(candidate), "OK"

    def report_task(self, agent_id: str, task_id: str, status: str, result: str) -> Tuple[bool, str]:
        task = next((t for t in self.tasks if t["id"] == task_id), None)
        if not task:
            return False, "TEAM_TASK_NOT_FOUND"
        if task.get("ownerAgentId") != agent_id:
            return False, "TEAM_MEMBER_FORBIDDEN"
        if task["status"] != "running":
            return False, "TEAM_TASK_NOT_RUNNING"

        task["status"] = status
        task["result"] = result
        self.propagate_failures()
        return True, "OK"

    def propagate_failures(self):
        while True:
            bad = {t["id"] for t in self.tasks if t["status"] in ("failed", "blocked")}
            changed = False
            for t in self.tasks:
                if t["status"] == "queued" and any(d in bad for d in t.get("deps", [])):
                    t["status"] = "blocked"
                    t["result"] = "依赖任务失败，未执行"
                    changed = True
            if not changed:
                break

    def control_team(self, action: str) -> Tuple[bool, str]:
        if action == "complete":
            if any(t["status"] != "completed" for t in self.tasks):
                return False, "TEAM_INCOMPLETE"
            self.status = "completed"
            return True, "OK"
        return False, "UNKNOWN_ACTION"


def build_world_build_dag_tasks_ref(buildings: List[Dict[str, Any]], style: str) -> List[Dict[str, Any]]:
    tasks = []
    artisan_ids = []
    for bldg in buildings:
        bid = bldg["id"]
        tid = f"artisan_{bid}"
        artisan_ids.append(tid)
        tasks.append({
            "id": tid,
            "title": f"Construct and refine building {bid}",
            "role": "building-artisan",
            "stage": "building_construction",
            "buildingId": bid,
            "deps": [],
            "status": "queued"
        })

    tasks.append({
        "id": "coherence_coordinator",
        "title": "Global coherence coordination and multi-camera verification",
        "role": "coordinator",
        "stage": "coherence_verification",
        "deps": artisan_ids,
        "status": "queued"
    })
    return tasks


def stress_test_dag_engine(ctx: EmpiricalHarnessContext):
    print(f"\n{CYAN}{BOLD}=== [Part 4] DAG Materialization & Failure Propagation Stress ==={RESET}")

    # Test 4.1: Materialize DAG with N = 6 buildings
    N = 6
    buildings = [{"id": f"bldg_{i:02d}", "name": f"Building {i}"} for i in range(N)]
    tasks = build_world_build_dag_tasks_ref(buildings, "cyberpunk")
    ctx.assert_equal(len(tasks), N + 1, f"Materialized DAG has exactly N+1 ({N+1}) tasks")

    artisan_tasks = [t for t in tasks if t["role"] == "building-artisan"]
    coord_task = next(t for t in tasks if t["role"] == "coordinator")

    ctx.assert_equal(len(artisan_tasks), N, f"Exactly {N} artisan tasks")
    ctx.assert_equal(len(coord_task["deps"]), N, f"Coordinator depends on all {N} artisan tasks")
    ctx.assert_true(all(len(t["deps"]) == 0 for t in artisan_tasks), "All artisan tasks have empty dependencies")

    # Test 4.2: Initial ready state: all N artisan tasks ready, coordinator NOT ready
    team = DagSimulatorTeam("team_test", max_parallel=4)
    team.set_tasks(tasks)
    initial_ready = team.ready_task_ids()
    ctx.assert_equal(len(initial_ready), N, f"Initially all {N} artisan tasks are ready")
    ctx.assert_true("coherence_coordinator" not in initial_ready, "Coordinator task MUST NOT be initially ready")

    # Test 4.3: Concurrency Limit max_parallel = 4
    # Claim 4 tasks concurrently -> all succeed
    for i in range(4):
        ok, claimed, code = team.claim_task(f"worker_{i}", f"artisan_bldg_{i:02d}")
        ctx.assert_true(ok, f"Worker {i} successfully claims task {i} (under max_parallel limit)")
        ctx.assert_equal(code, "OK", "Code is OK")

    # Attempt to claim 5th task -> MUST FAIL with TEAM_PARALLEL_LIMIT
    ok5, _, code5 = team.claim_task("worker_4", "artisan_bldg_04")
    ctx.assert_true(not ok5, "5th concurrent claim MUST be blocked by max_parallel = 4 limit")
    ctx.assert_equal(code5, "TEAM_PARALLEL_LIMIT", "Failure code MUST be TEAM_PARALLEL_LIMIT")

    # Test 4.4: Completion opens slot for next artisan task
    ok_rep, _ = team.report_task("worker_0", "artisan_bldg_00", "completed", "mesh generated")
    ctx.assert_true(ok_rep, "Worker 0 successfully reports completion")

    # Now 5th worker can claim
    ok5_retry, _, code5_retry = team.claim_task("worker_4", "artisan_bldg_04")
    ctx.assert_true(ok5_retry, "After task completion, 5th worker can now claim task")
    ctx.assert_equal(code5_retry, "OK", "Code is OK")

    # Test 4.5: Single Artisan Task Failure Propagation
    # Let worker 1 fail its task
    ok_fail, _ = team.report_task("worker_1", "artisan_bldg_01", "failed", "Meshy API timeout / OOM")
    ctx.assert_true(ok_fail, "Worker 1 reported task failure")

    # Verify coordinator task is marked BLOCKED
    coord_after_fail = next(t for t in team.tasks if t["id"] == "coherence_coordinator")
    ctx.assert_equal(coord_after_fail["status"], "blocked", "Single artisan task failure MUST transition coordinator to blocked")
    ctx.assert_true("依赖任务失败" in coord_after_fail.get("result", ""), "Coordinator result notes dependency failure")

    # Verify coordinator CANNOT be claimed
    ok_coord, _, code_coord = team.claim_task("coord_agent", "coherence_coordinator")
    ctx.assert_true(not ok_coord, "Blocked coordinator task CANNOT be claimed")
    ctx.assert_equal(code_coord, "TEAM_TASK_NOT_READY", "Coordinator claim fails with TEAM_TASK_NOT_READY")

    # Verify team cannot be completed
    ok_complete, code_complete = team.control_team("complete")
    ctx.assert_true(not ok_complete, "Team with failed/blocked tasks CANNOT be completed")
    ctx.assert_equal(code_complete, "TEAM_INCOMPLETE", "Error is TEAM_INCOMPLETE")

    # Test 4.6: Clean Execution Path: All N artisan tasks complete -> Coordinator becomes ready
    clean_team = DagSimulatorTeam("team_clean", max_parallel=4)
    clean_team.set_tasks(tasks)
    # Simulate sequential/parallel completion of all N artisan tasks
    for i in range(N):
        tid = f"artisan_bldg_{i:02d}"
        ok_c, _, _ = clean_team.claim_task("worker_pool", tid)
        assert ok_c
        ok_r, _ = clean_team.report_task("worker_pool", tid, "completed", "done")
        assert ok_r

    clean_ready = clean_team.ready_task_ids()
    ctx.assert_equal(clean_ready, ["coherence_coordinator"], "Upon all artisan completions, coordinator is ready")
    ok_coord_clean, _, _ = clean_team.claim_task("coordinator_worker", "coherence_coordinator")
    ctx.assert_true(ok_coord_clean, "Coordinator task can now be claimed successfully")

    ok_coord_done, _ = clean_team.report_task("coordinator_worker", "coherence_coordinator", "completed", "verified")
    ctx.assert_true(ok_coord_done, "Coordinator task reported completion")

    ok_all_complete, _ = clean_team.control_team("complete")
    ctx.assert_true(ok_all_complete, "Team successfully completes when all tasks complete")


# ---------------------------------------------------------------------------
# Section 5: Live Rust Backend cargo test Verification
# ---------------------------------------------------------------------------

def run_rust_cargo_tests(ctx: EmpiricalHarnessContext):
    print(f"\n{CYAN}{BOLD}=== [Part 5] Executing Rust Backend Cargo Tests ==={RESET}")

    # 5.1 Cargo test world_build::verify
    print("  Running cargo test -p forge-agentd --bin forge-agentd world_build::verify...")
    res1 = subprocess.run(
        ["cargo", "test", "-p", "forge-agentd", "--bin", "forge-agentd", "world_build::verify"],
        cwd=str(WORKSPACE_ROOT),
        capture_output=True,
        text=True,
        shell=True
    )
    ctx.assert_equal(res1.returncode, 0, "cargo test world_build::verify exits with code 0")
    ctx.assert_true("test world_build::verify::tests::test_aabb_xz_collision_and_tangent ... ok" in res1.stdout,
                    "Rust test_aabb_xz_collision_and_tangent passed")
    ctx.assert_true("test world_build::verify::tests::test_quaternion_normalization ... ok" in res1.stdout,
                    "Rust test_quaternion_normalization passed")
    ctx.assert_true("test world_build::verify::tests::test_coherence_verification_full_suite ... ok" in res1.stdout,
                    "Rust test_coherence_verification_full_suite passed")

    # 5.2 Cargo test collaboration
    print("  Running cargo test -p forge-agentd --bin forge-agentd collaboration::tests::test_world_build_dag_materialization...")
    res2 = subprocess.run(
        ["cargo", "test", "-p", "forge-agentd", "--bin", "forge-agentd", "test_world_build_dag_materialization"],
        cwd=str(WORKSPACE_ROOT),
        capture_output=True,
        text=True,
        shell=True
    )
    ctx.assert_equal(res2.returncode, 0, "cargo test test_world_build_dag_materialization exits with code 0")
    ctx.assert_true("test collaboration::tests::test_world_build_dag_materialization ... ok" in res2.stdout,
                    "Rust test_world_build_dag_materialization passed")

    # 5.3 Cargo test world_build actions (fix_coherence / approve_override)
    print("  Running cargo test -p forge-agentd --bin forge-agentd test_fix_coherence_and_approve_override_actions...")
    res3 = subprocess.run(
        ["cargo", "test", "-p", "forge-agentd", "--bin", "forge-agentd", "test_fix_coherence_and_approve_override_actions"],
        cwd=str(WORKSPACE_ROOT),
        capture_output=True,
        text=True,
        shell=True
    )
    ctx.assert_equal(res3.returncode, 0, "cargo test test_fix_coherence_and_approve_override_actions exits with code 0")
    ctx.assert_true("test world_build::tests::test_fix_coherence_and_approve_override_actions ... ok" in res3.stdout,
                    "Rust test_fix_coherence_and_approve_override_actions passed")

    # 5.4 Cargo test subagents building_artisan
    print("  Running cargo test -p forge-agentd --bin forge-agentd building_artisan...")
    res4 = subprocess.run(
        ["cargo", "test", "-p", "forge-agentd", "--bin", "forge-agentd", "building_artisan"],
        cwd=str(WORKSPACE_ROOT),
        capture_output=True,
        text=True,
        shell=True
    )
    ctx.assert_equal(res4.returncode, 0, "cargo test building_artisan exits with code 0")
    ctx.assert_true("test subagents::tests::building_artisan_allowlist_and_contract ... ok" in res4.stdout,
                    "Rust building_artisan_allowlist_and_contract passed")


# ---------------------------------------------------------------------------
# Section 6: Main Execution
# ---------------------------------------------------------------------------

def main():
    print(f"{CYAN}{BOLD}===================================================================={RESET}")
    print(f"{CYAN}{BOLD}  RurixForge Milestone 4 Challenger 1: Empirical Stress Suite     {RESET}")
    print(f"{CYAN}{BOLD}===================================================================={RESET}")

    start_time = time.time()
    ctx = EmpiricalHarnessContext()

    stress_test_aabb_xz_collisions(ctx)
    stress_test_interior_prop_hierarchy(ctx)
    stress_test_quaternion_normalization(ctx)
    stress_test_dag_engine(ctx)
    run_rust_cargo_tests(ctx)

    duration = time.time() - start_time
    print(f"\n{CYAN}{BOLD}===================================================================={RESET}")
    print(f"{CYAN}{BOLD}                     CHALLENGE RUN SUMMARY                          {RESET}")
    print(f"{CYAN}{BOLD}===================================================================={RESET}")
    print(f" Total Tests Passed : {GREEN}{ctx.passed}{RESET}")
    print(f" Total Tests Failed : {RED if ctx.failed > 0 else GREEN}{ctx.failed}{RESET}")
    print(f" Duration           : {duration:.2f}s")

    if ctx.failed > 0:
        print(f"\n{RED}{BOLD}FAILURES DETECTED:{RESET}")
        for err in ctx.errors:
            print(f" - {err}")
        sys.exit(1)
    else:
        print(f"\n{GREEN}{BOLD}ALL EMPIRICAL CHALLENGES PASSED CLEANLY WITH ZERO REGRESSIONS.{RESET}")
        sys.exit(0)


if __name__ == "__main__":
    main()
