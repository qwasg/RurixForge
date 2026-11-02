# RurixForge Build Composer Mode (`world_build`): Testing Infrastructure & E2E Specification

This document defines the testing philosophy, multi-tier test architecture, mathematical and geometric verification formulations, verification gates, feature coverage matrix, and test runner specifications for the **RurixForge Build Composer Mode (`world_build`)** and automated 3D scene construction pipeline.

---

## 1. Test Philosophy & Core Invariants

### 1.1 Opaque-Box & Requirement-Driven
Testing is conducted strictly as an **opaque-box (black-box)** verification against defined public interface contracts, objective file formats, mathematical formulations, and system outcomes. Tests MUST NOT rely on private implementation details, unexported helper functions, or ephemeral memory state. Every test assertion is derived directly from:
- `ORIGINAL_REQUEST.md` (Authoritative user requirements § 2026-10-17T12:30:32Z)
- `PROJECT.md` (System architecture, Feature Inventory, Interface Contracts, and Milestones)
- RurixForge Scene Contract (`crates/forge-scene`) and Asset Inspection Protocol (`assetd`)
- WebUI Preview Contract (`window.__buildPreview`) and Loopback Demo Host Protocol (`crates/forge-agentd/src/demo_host.rs`)
- Multi-Agent Team Collaboration Contract (`crates/forge-agentd/src/collaboration.rs`)

### 1.2 Anti-Facade & Zero-Mock Principle (Redline I-5)
1. **Real Geometric and Data Integrity**: Facade tests that trivially return `true` or mock away real transformation steps without executing real logic are strictly prohibited.
2. **Deterministic Geometry & Coherence Checks**: Tests must calculate actual bounding boxes, surface intersections on the $X-Z$ plane, hierarchical parent-child transforms, and pixel projections.
3. **Explicit Gate Blocking**: Overlapping building footprints, missing interior prop parents, unnormalized transforms, corrupted preview manifests, or invalid stage actions MUST be intercepted by the 5-Stage Verification Gates and fail fast with structured error codes.

### 1.3 Keystore Security & Secret Sanitization (Redline R-5)
1. **Zero Secret Leakage**: API tokens (such as Meshy API key `msy_...` or Antigravity OAuth tokens) MUST NEVER be hardcoded into git-tracked source code, test scripts, log outputs, or test artifacts.
2. **Environment & Keystore Resolution**: Keys must be read from the environment (`MESHY_API_KEY`) or the OS-encrypted DPAPI keystore (`data/keystore.json`).
3. **Response Sanitization**: All error messages, debug logs, and JSON reports pass through key redaction routines before emission.

---

## 2. Multi-Tier Testing Architecture

The test suite is structured into four distinct, progressive tiers spanning all 9 features from `PROJECT.md § Feature Inventory`:

```
+-----------------------------------------------------------------------------------+
|                        Tier 4: Real-World Application Scenarios                   |
|  - Full Lifecycle Workflows: Cyber Harbor, Medieval Castle, Sci-Fi Outpost, etc.  |
|  - Multi-Building Scene Assembly, Multi-Camera Viewport Frame Audit, Auto-Healing |
+-----------------------------------------+-----------------------------------------+
                                          ^
+-----------------------------------------+-----------------------------------------+
|                    Tier 3: Cross-Feature Integration Combinations                |
|  - Survey -> WebUI Preview -> Rebuild/Tune Actions -> Approval -> DAG -> Verify  |
|  - 4-Panel Sheet Vision -> Artisan Building Placement -> AABB Collision Remediation|
+-----------------------------------------+-----------------------------------------+
                                          ^
+-----------------------------------------+-----------------------------------------+
|                     Tier 2: Boundary, Adversarial & Gate Rejections               |
|  - Gate 1~5 Structural Failures, Out-of-Order Actions, Overlapping Footprints     |
|  - Malformed Payloads, Path Traversal, Coordinate Extremes, Nonexistent IDs       |
+-----------------------------------------+-----------------------------------------+
                                          ^
+-----------------------------------------+-----------------------------------------+
|                         Tier 1: Feature Contract Coverage                         |
|  - >= 5 Independent Test Cases Per Core Feature (Features 1 through 9)            |
|  - Schema Validation, State Machine Lifecycle, Rasterizer Math, DAG Materialization|
+-----------------------------------------------------------------------------------+
```

### Tier 1: Feature Contract Coverage (>= 45 tests)
Tests individual features in isolation against defined interface boundaries. Every feature has at least 5 distinct test cases covering nominal and variation cases:
- **Feature 1 (Mode & Protocol)**: Mode registration in `composerModes.ts`, `agent.rs` routing, request payload validation, allowed action vocabulary, non-interference with `id: 'build'`.
- **Feature 2 (Session & State Machine)**: 5-stage lifecycle (`survey` -> `preview_review` -> `construction` -> `coherence_verify` -> `done`), atomic file persistence in `.forge/world_build/<slug>/`, session crash recovery, fork safety.
- **Feature 3 (Project Survey Engine)**: Extraction of `forge.toml` settings, active `.rxscene` entities, `Light` components, `Camera` settings, materials, and art styles to `facts.json`.
- **Feature 4 (4-Panel 3-View Sheet)**: Pure CPU software rasterizer with z-buffer generating perspective showcase + front/side/top orthographic views, multimodal base64 return, bounding-box calculation.
- **Feature 5 (WebUI Preview & Probe)**: `demo_host` loopback hosting with 3D extension support, `window.__buildPreview` API implementation, headless web probe execution.
- **Feature 6 (Frontend Review Controls)**: Click-selection, targeted `rebuild_building`, `tune_building`, `adjust_style`, `approve_preview`, preserving non-targeted buildings.
- **Feature 7 (Multi-Agent Team & Artisan)**: Materialization of $N$-building DAG on `collaboration_runtime`, `building-artisan.md` profile, tool privileges, exterior/interior/texture tasks.
- **Feature 8 (Coherence Coordinator & Verifier)**: Server-side entity integrity, AABB footprint collision detection on $X-Z$ plane, interior prop `Parent` validation, multi-camera viewport verification.
- **Feature 9 (E2E Test Runner & Reporting)**: CLI interface, structured JSON execution report, clean exit codes, self-contained cleanup, zero secret leakage.

### Tier 2: Boundary, Adversarial & Gate Rejections (>= 45 tests)
Stress-tests the system with pathological, out-of-order, and adversarial inputs to guarantee robustness:
- Unrecognized or forbidden action payloads (HTTP 400 `WORLD_BUILD_INVALID_ACTION`).
- Out-of-sequence actions (e.g. `approve_preview` during `survey`) rejected with HTTP 409 `WORLD_BUILD_STAGE_MISMATCH`.
- Targeting nonexistent buildings (`buildingId`) rejected with HTTP 404 `WORLD_BUILD_BUILDING_NOT_FOUND`.
- Corrupt, empty, or degenerate 3D meshes handled without division-by-zero or crash.
- Security enforcement: loopback demo host rejects non-loopback Host headers and path traversal attempts.
- Footprint collisions: overlapping building AABBs on $X-Z$ plane rejected with `COHERENCE_COLLISION_DETECTED`.
- Orphaned interior props (missing `Parent` component or invalid parent reference) rejected.
- Adversarial strings (Unicode, null bytes, format meta-characters, extreme coordinate values) handled safely.

### Tier 3: Cross-Feature Integration Combinations (>= 9 tests)
Validates multi-component integration flows end-to-end:
- **Matrix 1**: Survey -> Preview Generation -> Building Selection -> Approval -> Multi-Agent DAG -> Verification.
- **Matrix 2**: Survey -> Preview -> Targeted Rebuild of Building A -> Style Adjustment -> Approval -> Coherence Verification.
- **Matrix 3**: 4-Panel Three-View Inspection Sheet -> Multimodal DataURL -> Building Artisan Prop Selection & Placement.
- **Matrix 4**: Loopback WebUI Preview -> Web Probe Automated Verification -> Interactive Selection -> Building Tuning -> Synchronized Manifest.
- **Matrix 5**: Preview Approval -> $N$-Building DAG Materialization -> Parallel Artisan Execution -> Interior Props Attachment -> Scene Save.
- **Matrix 6**: Multi-Building Scene Construction -> Coherence Verifier AABB Collision Detection -> Collision Remediation -> Final Verification.
- **Matrix 7**: Session Crash & Recovery -> Resume from `preview_review` stage -> Continue to Construction and Coherence Check.
- **Matrix 8**: Multi-Camera Viewport Capture -> Frustum Alignment -> Template View Comparison -> Audit Log Emission.
- **Matrix 9**: Composer Mode Request Dispatch -> `world_build` session resolution -> Atomic disk artifact updates across all 5 stages.
- **Matrix 10**: Secret Token Isolation across full pipeline: Meshy and Antigravity tokens stripped from all stage artifacts and reports.

### Tier 4: Real-World Application Scenarios (>= 5 scenarios)
Executes realistic multi-building game scene construction workflows:
- **Scenario 4.1 (Cyberpunk Harbor District)**: Multi-building harbor scene (warehouse, dock crane, shipping control tower, lighthouse) with neon signage, industrial lighting, water edge placement, and interior control room props.
- **Scenario 4.2 (Medieval Castle Courtyard)**: Fortress keep, guard towers, blacksmith forge, and outer walls with stone materials, torchlight lighting, interior anvil and weapon racks.
- **Scenario 4.3 (Sci-Fi Research Outpost)**: Modular habitat domes, solar array, communications dish, and landing pad on Martian terrain with low-gravity environment and interior lab equipment.
- **Scenario 4.4 (Fantasy Village Market Square)**: Tavern, merchant stalls, central fountain, and windmill with targeted building fine-tuning (`rebuild_building` on tavern roof, `tune_building` on merchant goods) and AABB non-collision.
- **Scenario 4.5 (High-Density Urban Skyway)**: Multi-story skyscrapers, connecting skybridges, rooftop helipads, and transit station with vertical AABB layering and multi-camera aerial viewpoints.
- **Scenario 4.6 (Adversarial Stress & Auto-Healing Scene)**: Intentionally injected overlapping buildings and orphaned interior props; validates coordinator error detection, targeted artisan fix rounds, and final verified resolution.

---

## 3. Mathematical & Algorithmic Specifications

### 3.1 2D AABB Footprint Collision Detection on the $X-Z$ Plane
To prevent buildings from unrealistically intersecting or overlapping on the terrain surface, Gate 5 evaluates the axis-aligned bounding box (AABB) projections of all building footprints onto the horizontal ground plane ($X-Z$).

For each building $A$ with world transform $(T_A, R_A, S_A)$ and local bounding box $[\mathbf{min}_A, \mathbf{max}_A]$:
1. The world-space horizontal extent is defined by:
   $$X_{\min}^A = T_{A,x} + \min_{c \in \text{Corners}} (c_x), \quad X_{\max}^A = T_{A,x} + \max_{c \in \text{Corners}} (c_x)$$
   $$Z_{\min}^A = T_{A,z} + \min_{c \in \text{Corners}} (c_z), \quad Z_{\max}^A = T_{A,z} + \max_{c \in \text{Corners}} (c_z)$$
2. Two buildings $A$ and $B$ collide if and only if their intervals overlap on both orthogonal axes:
   $$\text{Overlap}_{X}(A, B) = \max\left(0, \min\left(X_{\max}^A, X_{\max}^B\right) - \max\left(X_{\min}^A, X_{\min}^B\right)\right)$$
   $$\text{Overlap}_{Z}(A, B) = \max\left(0, \min\left(Z_{\max}^A, Z_{\max}^B\right) - \max\left(Z_{\min}^A, Z_{\min}^B\right)\right)$$
   $$\text{Collision}(A, B) \iff \left(\text{Overlap}_{X}(A, B) > \epsilon_{\text{col}}\right) \land \left(\text{Overlap}_{Z}(A, B) > \epsilon_{\text{col}}\right)$$
   where $\epsilon_{\text{col}} = 10^{-4}\,\text{m}$.

### 3.2 Pure CPU 4-Panel Software Rasterizer Projections
The asset inspection pipeline generates a 4-panel composite inspection sheet ($2 \times 2$ grid of width $W$, height $H$) containing:
- Panel $(0, 0)$: **Perspective Showcase View** (isometric angle $\theta_y = 45^\circ$, $\theta_x = 30^\circ$)
- Panel $(0, 1)$: **Front Orthographic View** (projection along $-Z$ onto $X-Y$)
- Panel $(1, 0)$: **Side Orthographic View** (projection along $+X$ onto $Z-Y$)
- Panel $(1, 1)$: **Top Orthographic View** (projection along $-Y$ onto $X-Z$)

Given model vertex $\mathbf{p} = [x, y, z]^T$, model center $\mathbf{c} = \frac{\mathbf{min} + \mathbf{max}}{2}$, and half-diagonal $R = \frac{\|\mathbf{max} - \mathbf{min}\|_2}{2}$:
1. **Centered Normalization**:
   $$\mathbf{p}_{\text{norm}} = \frac{\mathbf{p} - \mathbf{c}}{R}$$
2. **Orthographic Projection (Front)**:
   $$u = \frac{W_p}{2} \left(1.0 + \frac{x - c_x}{R}\right), \quad v = \frac{H_p}{2} \left(1.0 - \frac{y - c_y}{R}\right), \quad z_{\text{buf}} = \frac{z - c_z}{R}$$
3. **Perspective Projection**:
   $$\mathbf{p}_{\text{cam}} = \mathbf{R}_{\text{iso}} \cdot (\mathbf{p} - \mathbf{c}) + [0, 0, d_{\text{cam}}]^T$$
   $$u = \frac{W_p}{2} \left(1.0 + \frac{f \cdot p_{\text{cam},x}}{p_{\text{cam},z}}\right), \quad v = \frac{H_p}{2} \left(1.0 - \frac{f \cdot p_{\text{cam},y}}{p_{\text{cam},z}}\right)$$
   where $d_{\text{cam}} = 2.5 \cdot R$ and $f = 1.8$.
4. **Z-Buffer Interpolation**: Every rasterized pixel $(u, v)$ updates the color buffer only if $z < z_{\text{buf}}(u, v)$.

### 3.3 Multi-Camera Viewport Frustum & Angle Formulations
The coherence verifier positions virtual engine cameras according to WebUI template viewpoints:
1. **Camera View Vector**:
   $$\mathbf{f} = \frac{\mathbf{target} - \mathbf{pos}}{\|\mathbf{target} - \mathbf{pos}\|_2}, \quad \mathbf{r} = \frac{\mathbf{f} \times \mathbf{up}}{\|\mathbf{f} \times \mathbf{up}\|_2}, \quad \mathbf{u} = \mathbf{r} \times \mathbf{f}$$
2. **View Matrix**:
   $$\mathbf{V} = \begin{bmatrix} \mathbf{r}^T & -\mathbf{r} \cdot \mathbf{pos} \\ \mathbf{u}^T & -\mathbf{u} \cdot \mathbf{pos} \\ -\mathbf{f}^T & \mathbf{f} \cdot \mathbf{pos} \\ 0 & 1 \end{bmatrix}$$
3. Multi-camera angles include: Overview (`俯瞰全景`), Street Level (`街道透视`), and Hero Shot (`主建筑特写`).

### 3.4 Hierarchical Interior Prop Containment Equations
Interior decoration props must be physically contained within their parent building volume:
$$\forall p \in \text{InteriorProps}(B): \quad \mathbf{min}_{B} \le \mathbf{local\_pos}_p \le \mathbf{max}_{B}$$
and must reference the valid parent entity:
$$\text{Component}(p, \text{"Parent"}).\text{entity} == \text{EntityId}(B)$$

---

## 4. Five-Stage Verification Gates Specification

| Gate | Stage Name | Verifier Contract | Success Conditions | Structured Error Code |
|---|---|---|---|---|
| **Gate 1** | Survey & Mode Gate | `validate_gate_1_survey(session, facts)` | Mode is `world_build`; `facts.json` contains valid engine, render backend, lighting list, camera list, and materials; existing `build` mode unaffected | `GATE1_SURVEY_INVALID`, `GATE1_MODE_CONFLICT` |
| **Gate 2** | Preview & Contract Gate | `validate_gate_2_preview(manifest, probe)` | Preview manifest non-empty; `window.__buildPreview` contract exposes all required methods; building selection and review actions mutate targeted buildings and increment `rev` | `GATE2_PREVIEW_INVALID`, `GATE2_CONTRACT_VIOLATION` |
| **Gate 3** | Three-View Inspection Gate | `validate_gate_3_three_view(sheet_result)` | 4-panel composite PNG generated; valid base64 `dataUrl`; bounding box non-zero; all 4 quadrants contain rendered pixels | `GATE3_RASTERIZER_FAILED`, `GATE3_INVALID_ASSET` |
| **Gate 4** | Artisan DAG & Construction Gate | `validate_gate_4_dag(team_state)` | $N$ parallel building artisan tasks materialized; 1 dependent coordination task; artisan profiles adhere to tool whitelist | `GATE4_DAG_INVALID`, `GATE4_ARTISAN_FAILED` |
| **Gate 5** | Coherence & Viewport Gate | `validate_gate_5_coherence(scene, manifest)` | All $N$ buildings exist in scene; zero AABB footprint collisions; interior props attached via `Parent`; multi-camera viewport renders verify alignment | `GATE5_COHERENCE_COLLISION`, `GATE5_PROP_ORPHANED`, `GATE5_VIEWPORT_MISMATCH` |

---

## 5. Feature Inventory & Coverage Mapping

The following matrix maps every project feature from `PROJECT.md § Feature Inventory` across the test tiers:

| # | Feature Name | Tier 1 (Feature) | Tier 2 (Boundary) | Tier 3 (Cross-Combo) | Tier 4 (Real-World) |
|---|--------------|------------------|-------------------|----------------------|---------------------|
| 1 | Composer Mode Registration & Session Protocol | T1.F1.01-05 | T2.F1.01-05 | T3.CMB.01, T3.CMB.09 | T4.SCN.01, T4.SCN.04 |
| 2 | Session Persistence & State Machine Lifecycle | T1.F2.01-05 | T2.F2.01-05 | T3.CMB.01, T3.CMB.07 | T4.SCN.01, T4.SCN.05 |
| 3 | Automatic Project & Map Investigation Engine | T1.F3.01-05 | T2.F3.01-05 | T3.CMB.01, T3.CMB.02 | T4.SCN.01, T4.SCN.03 |
| 4 | Local Asset 4-Panel 3-View Showcase Sheet Pipeline | T1.F4.01-05 | T2.F4.01-05 | T3.CMB.03 | T4.SCN.02, T4.SCN.04 |
| 5 | Engine-Aligned WebUI 3D Preview Hosting & Web Probe | T1.F5.01-05 | T2.F5.01-05 | T3.CMB.04 | T4.SCN.01, T4.SCN.05 |
| 6 | Frontend Interactive Preview & Per-Building Controls | T1.F6.01-05 | T2.F6.01-05 | T3.CMB.02, T3.CMB.04 | T4.SCN.04, T4.SCN.06 |
| 7 | Multi-Agent Construction Team & Building Artisan | T1.F7.01-05 | T2.F7.01-05 | T3.CMB.05 | T4.SCN.01-06 |
| 8 | Global Coherence Coordinator & Multi-Camera Verification | T1.F8.01-05 | T2.F8.01-05 | T3.CMB.06, T3.CMB.08 | T4.SCN.01-06 |
| 9 | End-to-End Test Suite & Adversarial Hardening | T1.F9.01-05 | T2.F9.01-05 | T3.CMB.01-10 | T4.SCN.01-06 |

---

## 6. Test Runner Specification (`tools/e2e/test_world_build_e2e.py`)

### 6.1 CLI Arguments Interface
```bash
python tools/e2e/test_world_build_e2e.py [OPTIONS]

Options:
  --mode {all,tier1,tier2,tier3,tier4}
                        Test suite execution mode (default: all).
  --tier {1,2,3,4}      Run tests for a single specific tier.
  --output-report PATH  Path to write the output JSON execution report
                        (default: .forge/tmp/e2e/world_build_e2e_report.json).
  --collision-epsilon FLOAT
                        Minimum overlap threshold for AABB collision detection (default: 0.0001).
  --project-dir PATH    Root workspace directory (default: current working directory).
  --keep-artifacts      Retain temporary generated scenes, manifests, and cache files.
  --verbose, -v         Enable detailed step-by-step diagnostic logging.
```

### 6.2 Structured JSON Report Schema
```json
{
  "testRunId": "wb-e2e-20261017-130000-abcd",
  "timestamp": "2026-10-17T13:00:00Z",
  "runner": "tools/e2e/test_world_build_e2e.py",
  "mode": "all",
  "summary": {
    "totalTests": 106,
    "passed": 106,
    "failed": 0,
    "skipped": 0,
    "durationSeconds": 5.42
  },
  "gateResults": {
    "gate1_survey": { "status": "PASS", "evaluated": 12, "violations": 0 },
    "gate2_preview": { "status": "PASS", "evaluated": 14, "violations": 0 },
    "gate3_three_view": { "status": "PASS", "evaluated": 12, "violations": 0 },
    "gate4_dag_artisan": { "status": "PASS", "evaluated": 14, "violations": 0 },
    "gate5_coherence": { "status": "PASS", "evaluated": 18, "violations": 0 }
  },
  "tiers": {
    "tier1_features": { "total": 45, "passed": 45, "failed": 0 },
    "tier2_boundaries": { "total": 45, "passed": 45, "failed": 0 },
    "tier3_combinations": { "total": 10, "passed": 10, "failed": 0 },
    "tier4_scenarios": { "total": 6, "passed": 6, "failed": 0 }
  },
  "securityAudit": {
    "secretLeakageCheck": "CLEAN",
    "keystoreCompliant": true
  }
}
```

### 6.3 Exit Code Contract
- `0`: All requested tests passed; all gates cleared; collision checks validated; report written.
- `1`: One or more tests failed, or an assertion error was raised.
- `2`: Command line argument error or missing prerequisite.

---

## 7. Quality & Pass Criteria Thresholds

1. **Gate 1**: Mode `world_build` resolved without mutating `build` mode; project survey extracts valid engine, backend, lights, cameras, and materials.
2. **Gate 2**: WebUI preview manifests comply with `window.__buildPreview` contract; review actions (`rebuild`, `tune`, `style`) preserve non-targeted buildings and monotonically advance `rev`.
3. **Gate 3**: Pure CPU rasterizer produces valid 4-panel inspection sheets with non-empty pixels in all quadrants and correct bounding-box metadata.
4. **Gate 4**: Materialized DAG creates $N$ building artisan tasks with strict tool whitelists and 1 dependent global coherence task.
5. **Gate 5**: Server-side coherence verification confirms entity completeness, zero AABB footprint intersections, valid interior prop `Parent` bindings, and multi-camera viewport alignment.
6. **Security**: 0 occurrences of plaintext API keys or OAuth secrets in test reports, logs, or disk artifacts.
