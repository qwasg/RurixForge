# Project: RurixForge Build Composer Mode (`world_build`)

## Architecture
- **Composer Mode & Session State Machine Layer (`crates/forge-agentd/src/world_build.rs`, `sessions.rs`, `agent.rs`)**:
  - Registers `world_build` mode with UI label `Build` (preserving existing `id: 'build'`, label `Agent`).
  - `WorldBuildState` on `DebugSession` with atomic file-backed artifacts under `.forge/world_build/<slug>/` (`facts.json`, `manifest.json`, `template.json`, `scene.json`, `audit.json`).
  - Strict 5-stage lifecycle state machine: `survey` -> `preview_review` -> `construction` -> `coherence_verify` -> `done`.
  - Survey facts engine inspecting `forge.toml` (`3d`, `rurix`/`godot`), active `.rxscene` entities, active lighting (`Light` components), environment, cameras (`Camera`), materials (`.rxmat`), and art style.
- **Local Asset 4-Panel 3-View Showcase Sheet Pipeline (`crates/assetd`, `crates/mcp/asset-pipeline-mcp`)**:
  - Deterministic pure CPU software rasterizer with z-buffer for 3D meshes/models (`.glb`, `.rxmodel`, `.rxmesh`).
  - Generates a 4-panel composite inspection sheet: 1 perspective showcase view + front, side, and top orthographic views.
  - Cached under `.forge/cache/thumbs/<hash>.png`, returning `dataUrl: data:image/png;base64,...` (multimodal vision for LLM agents) and exact bounding-box / material metadata.
- **Loopback WebUI 3D Preview & Interactive Review (`crates/forge-agentd/src/demo_host.rs`, `web_probe.rs`, `packages/client`)**:
  - `demo_host.rs` loopback hosting with `.glb`, `.gltf`, `.bin` allowed.
  - Interactive Three.js/WebGL preview implementing `window.__buildPreview` (`getSceneState()`, `selectBuilding(id)`, `getSelectedBuilding()`, `applyStylePreset()`, `getCameraViews()`).
  - Automated verification via `web_probe.rs` and Playwright.
  - Synchronized frontend UI: `BuildPreviewCard` in chat timeline, `BuildPreviewTab` in Workbench, and `WorldBuildBar` in Composer.
  - Targeted review actions: `rebuild_building`, `tune_building`, `adjust_style`, and `approve_preview`, updating targeted buildings while incrementing `previewRev` and preserving other buildings.
- **Multi-Agent Engine Construction Team (`crates/forge-agentd/src/collaboration.rs`, `collaboration_runtime.rs`, `agents/building-artisan.md`)**:
  - Approved preview materializes an execution DAG on `collaboration_runtime.rs`: $N$ parallel building artisan tasks + 1 dependent global coherence coordination task.
  - Dedicated `building-artisan` profile (registered in `subagents.rs:BUILTIN_NAMES`) equipped with Meshy MCP tools (`gen_mesh`, `gen_mesh_refine`, `gen_accept`), local asset 3-view inspection, and engine scene tools.
- **Deterministic Server-Side Coherence Verification (`crates/forge-scene`, `crates/mcp/engine-scene-mcp`, `crates/forge-agentd/src/world_build/verify.rs`)**:
  - Verifies entity tree integrity and component validity in `.rxscene`.
  - AABB ground footprint intersection / collision detection on X-Z plane.
  - Verification of interior decoration props attached via `Parent` component to building entities.
  - Multi-camera engine screenshot captures via `viewport_set_camera` and `viewport_frame{exact: true}` with template view comparisons.

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | Composer Mode Registration & Session Protocol | Register `{ id: 'world_build', label: 'Build' }` in `composerModes.ts` & `agent.rs` without altering `{ id: 'build', label: 'Agent' }` | M1 | R1 |
| 2 | Session Persistence & State Machine Lifecycle | `WorldBuildState` on `DebugSession` with atomic storage in `.forge/world_build/<slug>/` and 5 stages (`survey` -> `preview_review` -> `construction` -> `coherence_verify` -> `done`) | M1 | R1 |
| 3 | Automatic Project & Map Investigation Engine | Inspect `forge.toml`, `.rxscene`, lighting, camera, environment, materials, models, and art style; persist `facts.json` | M1 | R1 |
| 4 | Local Asset 4-Panel 3-View Showcase Sheet Pipeline | Pure CPU rasterizer in `assetd`/`asset-pipeline-mcp` generating composite sheet (perspective + front/side/top) with multimodal base64 return & bounding-box metadata | M2 | R3 |
| 5 | Engine-Aligned WebUI 3D Preview Hosting & Web Probe | Add 3D extensions to `demo_host.rs`, implement `window.__buildPreview` contract, and automate verification via `web_probe.rs` | M3 | R2 |
| 6 | Frontend Interactive Preview & Per-Building Controls | `BuildPreviewCard` & `BuildPreviewTab`, click-selection, targeted `rebuild_building`, `tune_building`, `adjust_style`, `approve_preview` | M3 | R2 |
| 7 | Multi-Agent Construction Team & Building Artisan | Materialize $N$-building DAG in `collaboration.rs`; add `building-artisan.md` and update `BUILTIN_NAMES` in `subagents.rs` | M4 | R4 |
| 8 | Global Coherence Coordinator & Multi-Camera Verification | Server-side entity check, AABB collision detection, `Parent` interior prop check, and `viewport_frame` multi-camera comparison | M4 | R5 |
| 9 | End-to-End Test Suite & Adversarial Hardening | Comprehensive 4-tier requirement-driven E2E test suite + Tier 5 adversarial coverage hardening | M5 | AC |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M1 | Core Protocol, Session State Machine & Project Survey | `COMPOSER_MODES` update, `agent.rs` routing, `world_build.rs` module, `DebugSession` persistence, survey facts extraction, unit tests | none | DONE |
| M2 | Local Asset 4-Panel 3-View Showcase & Inspection MCP | Pure CPU 4-panel software rasterizer in `assetd`/`asset-pipeline-mcp`, thumbnail caching, multimodal base64 output, bounding metadata | none | DONE |
| M3 | Engine-Aligned WebUI 3D Preview, Web Probe & Review UI | `demo_host.rs` 3D types, `window.__buildPreview` contract, `web_probe.rs`, `BuildPreviewCard`/`Tab`, review actions (`rebuild`, `tune`, `style`, `approve`) | M1 | DONE |
| M4 | Multi-Agent Team Execution, Building Artisan & Coherence Gate | `building-artisan.md` profile, `BUILTIN_NAMES` update, DAG materialization, AABB collision check, `viewport_frame` engine verification | M1, M2, M3 | DONE |
| M5 | Final Milestone: 100% E2E Test Pass & Adversarial Hardening | Pass 100% E2E test suite (Tiers 1-4), Tier 5 adversarial coverage hardening, full regression pass | M1, M2, M3, M4, TEST_READY | DONE |

## Interface Contracts

### Composer Mode & Action Protocol
- Mode ID: `"world_build"`, UI Label: `"Build"`, Icon: `Hammer`
- Request Payload on `ask:execute`:
  ```json
  {
    "userInput": "Build a cyber harbor scene...",
    "mode": "world_build",
    "worldBuild": {
      "id": "slug",
      "rev": 1,
      "action": "survey" | "rebuild_building" | "tune_building" | "adjust_style" | "approve_preview",
      "buildingId": "bldg_dock_01",
      "instruction": "Add neon signs and weathered metal roof",
      "stylePreset": "cyberpunk_harbor"
    }
  }
  ```
- Error Responses:
  - HTTP 409 `WORLD_BUILD_STAGE_MISMATCH`
  - HTTP 404 `WORLD_BUILD_BUILDING_NOT_FOUND`
  - HTTP 400 `WORLD_BUILD_INVALID_ACTION`

### WebUI Preview Contract (`window.__buildPreview`)
```typescript
interface BuildPreviewApi {
  getSceneState(): {
    slug: string;
    rev: number;
    style: string;
    buildings: Array<{
      id: string;
      name: string;
      type: string;
      transform: { position: [number, number, number]; rotation: [number, number, number]; scale: [number, number, number] };
      bounds: { min: [number, number, number]; max: [number, number, number] };
      materials: string[];
      interiorPropsCount: number;
    }>;
    environment: { skybox: string; fog: boolean; ambientIntensity: number };
  };
  selectBuilding(id: string | null): boolean;
  getSelectedBuilding(): string | null;
  applyStylePreset(preset: string): boolean;
  getCameraViews(): Array<{ name: string; position: [number, number, number]; target: [number, number, number] }>;
}
```

### Local Asset Visual Inspection Tool Contract
- Tool Name: `mcp__asset-pipeline__inspect_3d_asset` (or extension to existing inspection tool)
- Input: `{ "path": "Content/Models/...", "generateThreeView": true }`
- Output:
  ```json
  {
    "path": "Content/Models/...",
    "polycount": 4250,
    "dimensions": { "x": 12.5, "y": 8.2, "z": 15.0 },
    "boundingBox": { "min": [-6.25, 0.0, -7.5], "max": [6.25, 8.2, 7.5] },
    "materials": ["mat_concrete", "mat_glass"],
    "threeViewSheetUri": ".forge/cache/thumbs/<hash>.png",
    "dataUrl": "data:image/png;base64,..."
  }
  ```

### Multi-Agent DAG Contract
- Stage 1 (`building_construction`): $N$ tasks with role `"building-artisan"`, each assigned `buildingId` and scope parameters.
- Stage 2 (`coherence_verification`): 1 task with role `"coordinator"`, depending on all $N$ building tasks, executing deterministic AABB collision check, hierarchy check, and `viewport_frame` multi-camera comparison.

## Code Layout
- `crates/forge-agentd/src/world_build.rs`: State machine, persistence, facts engine, review actions, coherence verifier.
- `crates/forge-agentd/src/world_build/`: Submodules for survey, facts, actions, and verification.
- `crates/forge-agentd/src/sessions.rs`: `world_build: Option<WorldBuildState>` on `DebugSession`.
- `crates/forge-agentd/src/agent.rs`: Mode resolution and routing.
- `crates/forge-agentd/src/demo_host.rs`: 3D file extensions for preview serving.
- `crates/assetd/src/rasterizer/`: Pure CPU 4-panel software rasterizer for 3D meshes.
- `crates/mcp/asset-pipeline-mcp/src/mcp.rs`: 3D model 3-view inspection tool endpoint.
- `agents/building-artisan.md`: Building artisan agent profile.
- `crates/forge-agentd/src/subagents.rs`: Subagent catalog whitelist updating `BUILTIN_NAMES`.
- `packages/client/src/components/chat/composerModes.ts`: Registering `world_build`.
- `packages/client/src/components/chat/world_build/`: `BuildPreviewCard.tsx`, `WorldBuildBar.tsx`.
- `packages/client/src/components/workbench/BuildPreviewTab.tsx`: 3D preview Workbench tab.
- `packages/client/src/lib/worldBuildStore.ts`: Client-side state store.
- `tools/e2e/world_build_pipeline.py`: Comprehensive E2E test runner.
