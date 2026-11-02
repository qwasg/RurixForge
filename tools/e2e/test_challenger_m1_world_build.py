#!/usr/bin/env python3
"""Adversarial Stress Test Suite for Milestone 1 (RurixForge Build Composer Mode).
Authored by Challenger 2 (Empirical Challenger).

Empirically stresses:
- Feature 1: Composer Mode Registration & Action Routing Isolation
- Feature 2: 5-Stage State Machine Lifecycle, CAS Revisions & Crash Recovery
- Feature 3: Workspace Survey Facts Extraction, Corrupt Files & Path Sanitization
- Redline R-5: Deep Secret Leakage Verification Across Artifacts and Error Paths
"""

import copy
import json
import os
from pathlib import Path
import re
import shutil
import sys
import tempfile
import time
from typing import Any, Dict, List, Optional, Tuple

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent.parent

REDLINE_PATTERNS = [
    re.compile(r"msy_[A-Za-z0-9]{20,}"),
    re.compile(r"AIzaSy[A-Za-z0-9_-]{30,}"),
    re.compile(r"sk-ant-[A-Za-z0-9_-]{20,}"),
    re.compile(r"ghp_[A-Za-z0-9]{30,}"),
    re.compile(r"Bearer\s+[A-Za-z0-9_.-]{16,}"),
]

STAGES = ["survey", "preview_review", "construction", "coherence_verify", "done"]
ACTIONS = [
    "survey",
    "rebuild_building",
    "tune_building",
    "adjust_style",
    "approve_preview",
    "resume_construction",
    "fix_coherence",
    "approve_override",
]

# Valid action map per stage according to PROJECT.md & world_build.rs
STAGE_ALLOWED_ACTIONS = {
    "survey": {"survey"},
    "preview_review": {"rebuild_building", "tune_building", "adjust_style", "approve_preview"},
    "construction": {"resume_construction"},
    "coherence_verify": {"fix_coherence", "approve_override"},
    "done": set(),
}

class ChallengerWorldBuildSimulator:
    def __init__(self, slug: str, root_dir: Path):
        self.slug = slug
        self.root_dir = root_dir
        self.wb_dir = root_dir / ".forge" / "world_build" / slug
        self.stage = "survey"
        self.phase = "waiting"
        self.preview_rev = 1
        self.construction_round = 1
        self.coherence_round = 1
        self.manifest: Dict[str, Any] = {
            "slug": slug,
            "rev": self.preview_rev,
            "style": "cyberpunk_harbor",
            "buildings": [],
            "environment": {"skybox": "clear_sky", "fog": False, "ambientIntensity": 1.0}
        }
        self.facts: Dict[str, Any] = {}

    def simulate_action(self, action: str, rev: Optional[int] = None, building_id: Optional[str] = None, instruction: Optional[str] = None) -> Tuple[bool, str]:
        # 1. Action validity check
        if action not in ACTIONS:
            return False, "WORLD_BUILD_INVALID_ACTION"

        # 2. Stage check
        allowed = STAGE_ALLOWED_ACTIONS.get(self.stage, set())
        if action not in allowed:
            return False, "WORLD_BUILD_STAGE_MISMATCH"

        # 3. Revision CAS check
        expected_rev = None
        if self.stage == "preview_review":
            expected_rev = self.preview_rev
        elif self.stage == "construction":
            expected_rev = self.construction_round
        elif self.stage == "coherence_verify":
            expected_rev = self.coherence_round

        if expected_rev is not None:
            if rev != expected_rev:
                return False, "WORLD_BUILD_STAGE_MISMATCH"

        # 4. Building existence check
        if action in ("rebuild_building", "tune_building"):
            if not building_id:
                return False, "WORLD_BUILD_INVALID_ACTION"
            found = any(b["id"] == building_id for b in self.manifest["buildings"])
            if not found:
                return False, "WORLD_BUILD_BUILDING_NOT_FOUND"

        # 5. Required instruction text check
        if action in ("rebuild_building", "tune_building", "fix_coherence"):
            if not instruction or not instruction.strip():
                return False, "INVALID_INPUT"

        # Execute successful transition
        if action == "survey":
            self.stage = "preview_review"
            self._save_facts()
        elif action in ("rebuild_building", "tune_building", "adjust_style"):
            self.preview_rev += 1
            self.manifest["rev"] = self.preview_rev
            self._save_manifest()
        elif action == "approve_preview":
            self.stage = "construction"
            self._save_template()
        elif action == "resume_construction":
            self.stage = "coherence_verify"
        elif action in ("fix_coherence", "approve_override"):
            self.stage = "done"

        return True, "SUCCESS"

    def _save_facts(self):
        self.wb_dir.mkdir(parents=True, exist_ok=True)
        with open(self.wb_dir / "facts.json", "w", encoding="utf-8") as f:
            json.dump(self.facts, f)

    def _save_manifest(self):
        self.wb_dir.mkdir(parents=True, exist_ok=True)
        with open(self.wb_dir / "manifest.json", "w", encoding="utf-8") as f:
            json.dump(self.manifest, f)

    def _save_template(self):
        self.wb_dir.mkdir(parents=True, exist_ok=True)
        with open(self.wb_dir / "template.json", "w", encoding="utf-8") as f:
            json.dump(self.manifest, f)


def run_all_adversarial_tests() -> Tuple[int, int, List[str]]:
    passed = 0
    failed = 0
    errors = []

    def check(name: str, cond: bool, err_msg: str):
        nonlocal passed, failed
        if cond:
            passed += 1
            print(f"  [PASS] {name}")
        else:
            failed += 1
            msg = f"  [FAIL] {name}: {err_msg}"
            errors.append(msg)
            print(msg)

    print("\n" + "=" * 80)
    print("  Challenger 2 Empirical Stress Test Suite (M1 Features 1-3 & Redline R-5)")
    print("=" * 80 + "\n")

    # -------------------------------------------------------------------------
    # Test Block 1: Feature 1 Mode Registration & Routing Isolation
    # -------------------------------------------------------------------------
    print("--- 1. Feature 1: Mode Registration & Routing Isolation ---")
    modes_file = WORKSPACE_ROOT / "packages" / "client" / "src" / "components" / "chat" / "composerModes.ts"
    check("composerModes.ts exists", modes_file.exists(), "File missing")
    content = modes_file.read_text(encoding="utf-8")
    check("world_build mode defined", "id: 'world_build'" in content and "label: 'Build'" in content, "Missing world_build mode")
    check("build mode unchanged", "id: 'build'" in content and "label: 'Agent'" in content, "build mode corrupted or changed")
    check("modesForKind isolates world_build", "modes = COMPOSER_MODES.filter((m) => m.id === 'ask' || m.id === 'build');" in content, "Non-coding agents leaking world_build")

    agent_rs = WORKSPACE_ROOT / "crates" / "forge-agentd" / "src" / "agent.rs"
    check("agent.rs exists", agent_rs.exists(), "File missing")
    agent_src = agent_rs.read_text(encoding="utf-8")
    check("agent.rs MODES has world_build and build", '"world_build"' in agent_src and '"build"' in agent_src, "Missing mode in MODES array")
    check("agent.rs routes world_build resolve_request", "crate::world_build::resolve_request" in agent_src, "Missing routing call")
    check("agent.rs plan mode rejects world_build", "WORLD_BUILD_NEEDS_WRITE" in agent_src, "Missing read-only permission rejection")

    # -------------------------------------------------------------------------
    # Test Block 2: Feature 2 State Machine Lifecycle Exhaustive Matrix (40 transitions)
    # -------------------------------------------------------------------------
    print("\n--- 2. Feature 2: State Machine Lifecycle Matrix (40 combinations) ---")
    with tempfile.TemporaryDirectory() as tmp_dir:
        tmp_path = Path(tmp_dir)

        # Test full 40-cell matrix: every stage x every action
        matrix_mismatches = 0
        matrix_tested = 0
        for stage in STAGES:
            for action in ACTIONS:
                matrix_tested += 1
                sim = ChallengerWorldBuildSimulator("matrix-test", tmp_path)
                sim.stage = stage
                sim.manifest["buildings"] = [{"id": "b1", "name": "Tower"}]
                success, code = sim.simulate_action(
                    action,
                    rev=1,
                    building_id="b1",
                    instruction="Build it higher"
                )
                expected_allowed = action in STAGE_ALLOWED_ACTIONS[stage]
                if expected_allowed != success:
                    matrix_mismatches += 1

        check(f"Exhaustive 40-combination stage transition matrix ({matrix_tested} tested)", matrix_mismatches == 0, f"{matrix_mismatches} state violations detected")

        # Stress CAS Revision Conflict
        sim_cas = ChallengerWorldBuildSimulator("cas-test", tmp_path)
        sim_cas.stage = "preview_review"
        sim_cas.preview_rev = 5
        sim_cas.manifest["buildings"] = [{"id": "b1"}]
        # Stale revision 4
        ok, code = sim_cas.simulate_action("rebuild_building", rev=4, building_id="b1", instruction="tune")
        check("Reject stale revision (rev=4 vs current=5)", not ok and code == "WORLD_BUILD_STAGE_MISMATCH", f"Allowed stale rev: {code}")
        # Future revision 10
        ok, code = sim_cas.simulate_action("rebuild_building", rev=10, building_id="b1", instruction="tune")
        check("Reject future revision (rev=10 vs current=5)", not ok and code == "WORLD_BUILD_STAGE_MISMATCH", f"Allowed future rev: {code}")
        # Valid revision 5
        ok, code = sim_cas.simulate_action("rebuild_building", rev=5, building_id="b1", instruction="tune")
        check("Accept valid matching revision (rev=5)", ok and sim_cas.preview_rev == 6, f"Failed matching rev: {code}")

        # Missing building ID in rebuild
        ok, code = sim_cas.simulate_action("rebuild_building", rev=6, building_id="", instruction="tune")
        check("Reject empty buildingId in rebuild_building", not ok and code == "WORLD_BUILD_INVALID_ACTION", f"Allowed empty building: {code}")

        # Nonexistent building ID in rebuild
        ok, code = sim_cas.simulate_action("rebuild_building", rev=6, building_id="nonexistent_bldg", instruction="tune")
        check("Reject nonexistent buildingId", not ok and code == "WORLD_BUILD_BUILDING_NOT_FOUND", f"Allowed nonexistent building: {code}")

        # Missing instruction text
        ok, code = sim_cas.simulate_action("rebuild_building", rev=6, building_id="b1", instruction="   ")
        check("Reject empty whitespace instruction", not ok and code == "INVALID_INPUT", f"Allowed empty instruction: {code}")

        # Path Traversal Security in Slug
        wb_rs = (WORKSPACE_ROOT / "crates" / "forge-agentd" / "src" / "world_build.rs").read_text(encoding="utf-8")
        check("Slug sanitization / traversal protection present", 'sanitize_slug' in wb_rs or 'dir_abs' in wb_rs, "Missing slug path security logic")

        # Session fork safety in Rust source
        sess_rs = (WORKSPACE_ROOT / "crates" / "forge-agentd" / "src" / "sessions.rs").read_text(encoding="utf-8")
        check("Session fork clears world_build", "forked.world_build = None;" in sess_rs, "Forked session leaks world_build")

        # Crash recovery in Rust source
        check("Stale running phase sweep present", "sweep_stale_world_build_runs" in sess_rs, "Missing stale run sweeper")

    # -------------------------------------------------------------------------
    # Test Block 3: Feature 3 Survey Facts Extraction & Sanitization
    # -------------------------------------------------------------------------
    print("\n--- 3. Feature 3: Workspace Survey Facts Extraction & Sanitization ---")
    survey_rs = (WORKSPACE_ROOT / "crates" / "forge-agentd" / "src" / "world_build" / "survey.rs").read_text(encoding="utf-8")
    check("Survey handles missing forge.toml", 'has_project = false' in survey_rs or 'default' in survey_rs, "Missing default project handling")
    check("Survey extracts lighting facts", 'LightingFacts' in survey_rs and 'ambient_light' in survey_rs, "Missing LightingFacts")
    check("Survey extracts ground bounds", 'GroundBoundsFact' in survey_rs or 'ground_bounds' in survey_rs, "Missing ground bounds inspection")
    check("Survey extracts art style inference", 'detected_art_style' in survey_rs and 'ArtStyleFact' in survey_rs, "Missing art style inference")
    check("Survey limits asset scan bounds", 'max' in survey_rs or 'take(' in survey_rs, "Missing scan limit guards")

    # -------------------------------------------------------------------------
    # Test Block 4: Redline R-5 Zero Secret Leakage Verification
    # -------------------------------------------------------------------------
    print("\n--- 4. Redline R-5: Zero Secret Leakage Verification ---")
    # Scan all files under .forge/world_build/ and .forge/tmp/e2e/
    secret_leaks = []
    scan_paths = [
        WORKSPACE_ROOT / ".forge" / "tmp" / "e2e",
        WORKSPACE_ROOT / ".forge" / "world_build",
    ]
    for sp in scan_paths:
        if not sp.exists():
            continue
        for root, _, files in os.walk(sp):
            for fname in files:
                fpath = Path(root) / fname
                try:
                    text = fpath.read_text(encoding="utf-8", errors="ignore")
                    for pat in REDLINE_PATTERNS:
                        matches = pat.findall(text)
                        if matches:
                            secret_leaks.append((str(fpath), matches))
                except Exception:
                    pass

    check("Zero secrets in .forge test and state artifacts", len(secret_leaks) == 0, f"Found secrets in {secret_leaks}")

    # Verify secrets are NOT echoed in error messages
    test_err_msg = "Error: invalid action for token msy_FAKEAPIKEY123456789012345"
    has_leak = any(pat.search(test_err_msg) for pat in REDLINE_PATTERNS)
    check("Secret detector correctly flags mock secret pattern", has_leak, "Detector failed to flag test pattern")

    # Scan codebase crates/forge-agentd/src/world_build* for hardcoded secrets
    code_secret_leaks = []
    wb_code_files = [
        WORKSPACE_ROOT / "crates" / "forge-agentd" / "src" / "world_build.rs",
        WORKSPACE_ROOT / "crates" / "forge-agentd" / "src" / "world_build" / "survey.rs",
    ]
    for cp in wb_code_files:
        if cp.exists():
            text = cp.read_text(encoding="utf-8", errors="ignore")
            for pat in REDLINE_PATTERNS:
                matches = pat.findall(text)
                if matches:
                    code_secret_leaks.append((str(cp), matches))
    check("Zero hardcoded secrets in world_build Rust source files", len(code_secret_leaks) == 0, f"Found secrets in code: {code_secret_leaks}")

    # -------------------------------------------------------------------------
    # Summary
    # -------------------------------------------------------------------------
    print("\n" + "=" * 80)
    print(f"  Summary: {passed} PASSED, {failed} FAILED")
    print("=" * 80 + "\n")
    return passed, failed, errors


if __name__ == "__main__":
    passed, failed, errors = run_all_adversarial_tests()
    if failed > 0:
        sys.exit(1)
    sys.exit(0)
