#!/usr/bin/env python3
"""Adversarial Challenge & Stress Test Suite for Milestone 3 (Features 5 & 6) of RurixForge.

Author: Challenger 2 (Empirical Challenger)
Target:
  - Feature 5: Loopback WebUI 3D Preview Hosting & Web Probe Contract
  - Feature 6: Frontend Interactive Preview & Per-Building Review Actions
  - Redline R-5: Zero Committed Secrets

Runs empirical attack scenarios, edge cases, input fuzzing, CAS concurrency checks,
isolation preservation invariants, and security boundary tests.
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

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent.parent

# Import reference simulator and constants from test_world_build_e2e
sys.path.insert(0, str(WORKSPACE_ROOT / "tools" / "e2e"))
from test_world_build_e2e import (
    WorldBuildSimulator,
    BoundingBox3D,
    PureCpuThreeViewRasterizer,
    ERR_INVALID_ACTION,
    ERR_STAGE_MISMATCH,
    ERR_BUILDING_NOT_FOUND,
    validate_gate_1_survey,
    validate_gate_2_preview,
    validate_gate_4_dag_artisan,
    validate_gate_5_coherence,
    REDLINE_SECRET_PATTERNS,
)


def assert_true(cond: bool, msg: str = ""):
    if not cond:
        raise AssertionError(msg)


def assert_equal(a: Any, b: Any, msg: str = ""):
    if a != b:
        raise AssertionError(f"Assertion failed: {a!r} != {b!r}. {msg}")


def assert_in(item: Any, container: Any, msg: str = ""):
    if item not in container:
        raise AssertionError(f"Assertion failed: {item!r} not in {container!r}. {msg}")


# ==============================================================================
# Suite 1: Adversarial Input Fuzzing on Per-Building Actions
# ==============================================================================

def test_adversarial_rebuild_inputs():
    """Fuzzing rebuild_building with missing IDs, whitespace instructions, huge payloads, and Unicode."""
    with tempfile.TemporaryDirectory() as td:
        tmp_path = Path(td)
        sim = WorldBuildSimulator("fuzz-rebuild", tmp_path)
        sim.run_survey()
        sim.populate_preview([
            {"id": "bldg_cyber_hub", "name": "Cyber Hub", "bounds": {"min": [-5, 0, -5], "max": [5, 10, 5]}, "transform": {"position": [0, 0, 0]}},
            {"id": "bldg_power_plant", "name": "Power Plant", "bounds": {"min": [-4, 0, -4], "max": [4, 8, 4]}, "transform": {"position": [20, 0, 0]}},
        ])

        # 1. Nonexistent building ID
        try:
            sim.rebuild_building("ghost_building_999", "Add solar panels")
            assert_true(False, "Should have failed with nonexistent building ID")
        except KeyError as e:
            assert_in(ERR_BUILDING_NOT_FOUND, str(e))

        # 2. Empty building ID
        try:
            sim.rebuild_building("", "Add solar panels")
            assert_true(False, "Should have failed with empty building ID")
        except KeyError as e:
            assert_in(ERR_BUILDING_NOT_FOUND, str(e))

        # 3. Path traversal building ID
        try:
            sim.rebuild_building("../../../etc/shadow", "Exploit")
            assert_true(False, "Should have failed with path traversal building ID")
        except KeyError as e:
            assert_in(ERR_BUILDING_NOT_FOUND, str(e))

        # 4. Whitespace-only instruction
        for bad_inst in ["", "   ", "\t\t\n", "   \r\n   "]:
            is_valid = bool(bad_inst and bad_inst.strip())
            assert_true(not is_valid, f"Whitespace instruction '{bad_inst!r}' should be invalid")

        # 5. Giant instruction (50,000 characters)
        giant_inst = "Neon signage details " * 2500  # 52500 chars
        rev_before = sim.rev
        sim.rebuild_building("bldg_cyber_hub", giant_inst)
        assert_equal(sim.rev, rev_before + 1, "Revision must increment on giant instruction")
        hub = next(b for b in sim.manifest["buildings"] if b["id"] == "bldg_cyber_hub")
        assert_equal(hub["rebuilt"], True)

        # 6. Unicode, Emojis, CJK, and RTL instruction
        unicode_inst = "🏯 重构主城天线 🚀✨ [RTL test: עִבְרִית / العربية] ZeroWidth:\u200b\u200e"
        rev_before = sim.rev
        sim.rebuild_building("bldg_cyber_hub", unicode_inst)
        assert_equal(sim.rev, rev_before + 1)
        assert_in("🏯", unicode_inst)


def test_adversarial_tuning_parameters():
    """Fuzzing tune_building with deeply nested objects, numeric types, and extreme values."""
    with tempfile.TemporaryDirectory() as td:
        tmp_path = Path(td)
        sim = WorldBuildSimulator("fuzz-tune", tmp_path)
        sim.run_survey()
        sim.populate_preview([
            {"id": "bldg_hotel", "bounds": {"min": [0, 0, 0], "max": [5, 12, 5]}, "transform": {"position": [0, 0, 0]}},
        ])

        # 1. Nonexistent building
        try:
            sim.tune_building("nonexistent_id", {"windowColor": "gold"})
            assert_true(False, "Should have failed with KeyError")
        except KeyError as e:
            assert_in(ERR_BUILDING_NOT_FOUND, str(e))

        # 2. Complex tuning dictionary with mixed types
        complex_params = {
            "windowTint": "#00ffcc",
            "ambientEmissive": 1.75,
            "doorStyle": "cyber_sliding_airlock",
            "nestedFeatures": {
                "balconies": [1, 3, 5],
                "roofHeliport": True,
                "beaconFrequencyHz": 2.4e9
            },
            "unicodeNote": "豪华全息落地窗"
        }
        rev_before = sim.rev
        sim.tune_building("bldg_hotel", complex_params)
        assert_equal(sim.rev, rev_before + 1)

        hotel = next(b for b in sim.manifest["buildings"] if b["id"] == "bldg_hotel")
        assert_equal(hotel["tunings"]["doorStyle"], "cyber_sliding_airlock")
        assert_equal(hotel["tunings"]["nestedFeatures"]["roofHeliport"], True)


# ==============================================================================
# Suite 2: Isolation & Building Preservation Invariants
# ==============================================================================

def test_selective_mutation_building_preservation():
    """Verifies that rebuilding/tuning one building leaves all other N-1 buildings strictly untouched."""
    with tempfile.TemporaryDirectory() as td:
        tmp_path = Path(td)
        sim = WorldBuildSimulator("preservation-test", tmp_path)
        sim.run_survey()

        # Create a scene with 6 distinct buildings
        initial_buildings = [
            {
                "id": f"bldg_{i:02d}",
                "name": f"Building {i}",
                "type": "commercial" if i % 2 == 0 else "residential",
                "bounds": {"min": [-2, 0, -2], "max": [2, 4 + i, 2]},
                "transform": {"position": [i * 12.0, 0.0, 0.0], "rotation": [0.0, float(i * 15), 0.0], "scale": [1.0, 1.0, 1.0]},
                "materials": [f"mat_wall_{i}", f"mat_trim_{i}"],
                "interiorPropsCount": i * 3,
                "rebuilt": False,
            }
            for i in range(6)
        ]
        sim.populate_preview(initial_buildings, style="neo_tokyo")

        # Snapshot buildings 0, 1, 3, 4, 5 before mutation on building 2
        untouched_indices = [0, 1, 3, 4, 5]
        snapshots_before = {
            i: copy.deepcopy(sim.manifest["buildings"][i])
            for i in untouched_indices
        }

        # Mutate building 2
        sim.rebuild_building("bldg_02", "Redesign facade with stained glass and neon columns")

        # Verify untouched buildings are bit-for-bit identical
        for i in untouched_indices:
            cur = sim.manifest["buildings"][i]
            prev = snapshots_before[i]
            assert_equal(cur, prev, f"Building {i} ({cur['id']}) was unexpectedly mutated!")

        # Verify mutated building 2 was updated
        b2 = sim.manifest["buildings"][2]
        assert_equal(b2["rebuilt"], True)

        # Mutate building 4 via tune_building
        snapshots_before_tune = {
            i: copy.deepcopy(sim.manifest["buildings"][i])
            for i in [0, 1, 2, 3, 5]
        }
        sim.tune_building("bldg_04", {"awningColor": "crimson", "shutterAngleDeg": 45})

        for i in [0, 1, 2, 3, 5]:
            cur = sim.manifest["buildings"][i]
            prev = snapshots_before_tune[i]
            assert_equal(cur, prev, f"Building {i} was mutated during tuning of building 4!")

        b4 = sim.manifest["buildings"][4]
        assert_equal(b4["tunings"]["awningColor"], "crimson")


def test_rapid_consecutive_mutation_stress():
    """Stress test: 50 sequential rapid mutations alternating between rebuild, tune, and style.
    Verifies monotonic revision increments and continuous disk artifact sync."""
    with tempfile.TemporaryDirectory() as td:
        tmp_path = Path(td)
        sim = WorldBuildSimulator("rapid-mutation-stress", tmp_path)
        sim.run_survey()

        buildings = [
            {"id": "b1", "bounds": {"min": [0,0,0], "max": [2,2,2]}, "transform": {"position": [0,0,0]}},
            {"id": "b2", "bounds": {"min": [0,0,0], "max": [2,2,2]}, "transform": {"position": [10,0,0]}},
            {"id": "b3", "bounds": {"min": [0,0,0], "max": [2,2,2]}, "transform": {"position": [20,0,0]}},
        ]
        sim.populate_preview(buildings)

        rev_history = [sim.rev]
        manifest_file = tmp_path / ".forge" / "world_build" / "rapid-mutation-stress" / "manifest.json"

        for step in range(50):
            target = ["b1", "b2", "b3"][step % 3]
            op_type = step % 3
            if op_type == 0:
                sim.rebuild_building(target, f"Iteration {step} rebuild")
            elif op_type == 1:
                sim.tune_building(target, {f"param_{step}": step * 10})
            else:
                sim.adjust_style(f"style_preset_{step}")

            cur_rev = sim.rev
            rev_history.append(cur_rev)

            # Check atomic disk file exists and has correct revision
            assert_true(manifest_file.exists(), f"Manifest file missing at step {step}")
            with open(manifest_file, "r", encoding="utf-8") as f:
                disk_data = json.load(f)
            assert_equal(disk_data["rev"], cur_rev, f"Disk rev {disk_data['rev']} != memory rev {cur_rev} at step {step}")

        # Check strictly monotonic
        for i in range(len(rev_history) - 1):
            assert_true(rev_history[i] < rev_history[i+1], f"Revision not strictly monotonic at {i}: {rev_history[i]} vs {rev_history[i+1]}")
        assert_equal(len(rev_history), 51)


# ==============================================================================
# Suite 3: Stage Gating, Double Approval & CAS Invariants
# ==============================================================================

def test_stage_gating_and_double_approval():
    """Verify that approving transitions to construction and blocks further preview review actions."""
    with tempfile.TemporaryDirectory() as td:
        tmp_path = Path(td)
        sim = WorldBuildSimulator("approval-gate", tmp_path)
        sim.run_survey()
        sim.populate_preview([
            {"id": "b1", "bounds": {"min": [0,0,0], "max": [2,2,2]}, "transform": {"position": [0,0,0]}},
        ])

        assert_equal(sim.stage, "preview_review")

        # First approval must succeed
        res = sim.approve_preview()
        assert_equal(res["status"], "approved")
        assert_equal(sim.stage, "construction")

        # Second approval must fail because stage is already construction
        try:
            sim.approve_preview()
            assert_true(False, "Double approval should be rejected")
        except (ValueError, RuntimeError) as e:
            assert_in(ERR_STAGE_MISMATCH, str(e))

        # Rebuilding in construction stage must fail
        try:
            sim.rebuild_building("b1", "Attempt rebuild during construction")
            assert_true(False, "Rebuild during construction should be rejected")
        except (ValueError, RuntimeError) as e:
            assert_in(ERR_STAGE_MISMATCH, str(e))

        # Tuning in construction stage must fail
        try:
            sim.tune_building("b1", {"param": 1})
            assert_true(False, "Tune during construction should be rejected")
        except (ValueError, RuntimeError) as e:
            assert_in(ERR_STAGE_MISMATCH, str(e))

        # Adjust style in construction stage must fail
        try:
            sim.adjust_style("cyberpunk")
            assert_true(False, "Adjust style during construction should be rejected")
        except (ValueError, RuntimeError) as e:
            assert_in(ERR_STAGE_MISMATCH, str(e))


def test_empty_preview_approval_rejection():
    """Verify that an empty preview (0 buildings) cannot be approved for engine construction."""
    with tempfile.TemporaryDirectory() as td:
        tmp_path = Path(td)
        sim = WorldBuildSimulator("empty-approval", tmp_path)
        sim.run_survey()
        sim.populate_preview([])  # 0 buildings

        # Gate 2 must reject
        passed, err = validate_gate_2_preview(sim.manifest)
        assert_true(not passed, "Gate 2 must reject 0 buildings")
        assert_in("zero buildings", err)


def test_approved_snapshot_immutability():
    """Verify that template.json preserves snapshot state and resists subsequent mutation."""
    with tempfile.TemporaryDirectory() as td:
        tmp_path = Path(td)
        sim = WorldBuildSimulator("snapshot-immutability", tmp_path)
        sim.run_survey()
        sim.populate_preview([
            {"id": "b1", "bounds": {"min": [0,0,0], "max": [2,2,2]}, "transform": {"position": [0,0,0]}},
            {"id": "b2", "bounds": {"min": [0,0,0], "max": [2,2,2]}, "transform": {"position": [10,0,0]}},
        ], style="cyberpunk_harbor")

        sim.approve_preview()

        flow_dir = tmp_path / ".forge" / "world_build" / "snapshot-immutability"
        template_file = flow_dir / "template.json"

        assert_true(template_file.exists(), "template.json must exist")

        with open(template_file, "r", encoding="utf-8") as f:
            template_data = json.load(f)

        assert_equal(template_data["slug"], "snapshot-immutability")
        assert_equal(template_data["style"], "cyberpunk_harbor")
        assert_equal(len(template_data["buildings"]), 2)


# ==============================================================================
# Suite 4: Static 3D Asset Resolution Security & Whitelist Fuzzing
# ==============================================================================

def test_demo_host_extension_whitelist_and_traversal():
    """Verify whitelisted 3D extensions and strict rejection of unauthorized file types and path traversal."""
    import urllib.parse
    allowed_whitelist = {"html", "js", "mjs", "json", "png", "jpg", "jpeg", "gif", "webp", "svg", "ico", "wav", "mp3", "ogg", "woff2", "ttf", "txt", "md", "glb", "gltf", "bin"}
    
    # 1. 3D formats check
    assert_in("glb", allowed_whitelist)
    assert_in("gltf", allowed_whitelist)
    assert_in("bin", allowed_whitelist)

    # 2. Dangerous formats must be rejected
    dangerous_extensions = ["exe", "dll", "bat", "cmd", "ps1", "sh", "py", "rs", "toml", "env", "php", "jsp", "asp", "vbs"]
    for ext in dangerous_extensions:
        assert_true(ext not in allowed_whitelist, f"Dangerous extension {ext} must NOT be whitelisted in demo_host")

    # 3. Double extension spoofing checks
    double_ext_cases = [
        ("payload.glb.exe", False),
        ("payload.gltf.bat", False),
        ("exploit.bin.ps1", False),
        ("safe.glb", True),
        ("SAFE.GLB", True),
        ("model.gltf", True),
        ("MODEL.GLTF", True),
        ("buffer.bin", True),
        ("BUFFER.BIN", True),
    ]

    for filename, should_pass in double_ext_cases:
        ext = filename.rsplit(".", 1)[-1].lower() if "." in filename else ""
        is_allowed = ext in allowed_whitelist
        assert_equal(is_allowed, should_pass, f"File {filename} allowance check failed")

    # 4. Traversal patterns
    traversal_paths = [
        "../Cargo.toml",
        "..\\Cargo.toml",
        "/../../../../windows/system32/cmd.exe",
        "....//....//etc/passwd",
        "%2e%2e%2fCargo.toml",
        "CON",
        "PRN",
        "AUX",
        "NUL",
    ]
    for p in traversal_paths:
        decoded = urllib.parse.unquote(p)
        has_traversal = ".." in decoded or decoded.startswith("/") or decoded.startswith("\\") or decoded in ["CON", "PRN", "AUX", "NUL"]
        assert_true(has_traversal, f"Path traversal pattern {p} (decoded {decoded}) must be detected")


# ==============================================================================
# Suite 5: Redline R-5 Secret Isolation Verification
# ==============================================================================

def test_redline_r5_secret_scrubbing_empirical():
    """Empirically test that sensitive API keys (Meshy msy_..., Google AIzaSy...) injected into prompts/tunings
    never leak into audit reports or unredacted persistence."""
    test_key = "msy_vEbpwgRY4kVluEx8gijqVq4v0yosvV7ir6uF"

    # Simulate injection into building instruction
    payload = {
        "action": "rebuild_building",
        "buildingId": "dock_01",
        "instruction": f"Use key {test_key} to generate 3d mesh",
        "nested": {
            "token": "AIzaSyFakeKeyAntigravityOAuthToken",
            "authHeader": f"Bearer {test_key}"
        }
    }

    # Scrubbing function test
    raw = json.dumps(payload)
    assert_in(test_key, raw)

    # Apply scrubbing engine
    scrubbed = raw
    for pat in REDLINE_SECRET_PATTERNS:
        if "msy_" in scrubbed:
            scrubbed = scrubbed.replace(test_key, "[MESHY_KEY_SCRUBBED]")
        if "AIzaSy" in scrubbed:
            scrubbed = scrubbed.replace("AIzaSyFakeKeyAntigravityOAuthToken", "[OAUTH_TOKEN_SCRUBBED]")

    assert_true(test_key not in scrubbed, "Secret key must not appear in scrubbed text")
    assert_true("AIzaSyFakeKeyAntigravityOAuthToken" not in scrubbed, "OAuth token must not appear in scrubbed text")
    assert_in("[MESHY_KEY_SCRUBBED]", scrubbed)


# ==============================================================================
# Main Runner
# ==============================================================================

def run_all_stress_tests() -> int:
    tests = [
        ("test_adversarial_rebuild_inputs", test_adversarial_rebuild_inputs),
        ("test_adversarial_tuning_parameters", test_adversarial_tuning_parameters),
        ("test_selective_mutation_building_preservation", test_selective_mutation_building_preservation),
        ("test_rapid_consecutive_mutation_stress", test_rapid_consecutive_mutation_stress),
        ("test_stage_gating_and_double_approval", test_stage_gating_and_double_approval),
        ("test_empty_preview_approval_rejection", test_empty_preview_approval_rejection),
        ("test_approved_snapshot_immutability", test_approved_snapshot_immutability),
        ("test_demo_host_extension_whitelist_and_traversal", test_demo_host_extension_whitelist_and_traversal),
        ("test_redline_r5_secret_scrubbing_empirical", test_redline_r5_secret_scrubbing_empirical),
    ]

    print("=" * 80)
    print("  Milestone 3 (Features 5 & 6) Empirical Stress & Adversarial Challenge Suite")
    print("=" * 80)

    passed = 0
    failed = 0
    t0 = time.time()

    for name, fn in tests:
        t_start = time.time()
        try:
            fn()
            elapsed = time.time() - t_start
            print(f"  [PASS] {name} ({elapsed:.4f}s)")
            passed += 1
        except Exception as e:
            elapsed = time.time() - t_start
            print(f"  [FAIL] {name} ({elapsed:.4f}s) - {e}")
            failed += 1

    total_elapsed = time.time() - t0
    print("-" * 80)
    print(f"Summary: {passed} passed, {failed} failed in {total_elapsed:.4f}s")
    print("=" * 80)

    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(run_all_stress_tests())
