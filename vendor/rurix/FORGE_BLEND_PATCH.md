# Forge RasterPass alpha/additive patch

This directory vendors the **rurix-rt 1.0.0** package from [qwasg/Rurix](https://github.com/qwasg/Rurix) revision **1478859a8f2ea3a8e17abb06aa0211a6e0871cca**, the exact revision already pinned by RurixForge. The upstream MIT and Apache-2.0 licenses are retained.

The root Cargo.toml now applies the repository-local patch:

```toml
[patch."https://github.com/qwasg/Rurix"]
rurix-rt = { path = "vendor/rurix/src/rurix-rt" }
```

No user-specific path and no modified Cargo cache or D:/Rurix working tree is needed. All package/build/dev dependencies of this package still point to the same pinned upstream Git revision. The package manifest has explicit edition/version/license values and an isolated workspace. Required upstream build-input directories conformance/vulkan/accept and conformance/dxil/graphics/accept are copied verbatim so build.rs retains its original inputs.

## Problem and actual fix

Previously, RasterPass had no blending field and Vulkan pipeline creation always used blend_enable=0. The Sprite fragment shader also ignored texture alpha and unconditionally discarded magenta pixels. Returning an alpha value alone cannot repair framebuffer composition.

The patch adds public BlendMode::{Opaque, Alpha, Additive} to RasterPass and includes the mode in RasterPipelineKey:

- Opaque keeps blending disabled.
- Alpha uses RGB source SRC_ALPHA and destination ONE_MINUS_SRC_ALPHA; alpha uses ONE and ONE_MINUS_SRC_ALPHA.
- Additive uses RGB source SRC_ALPHA and destination ONE; alpha uses ZERO and ONE to preserve destination alpha.
- Blended passes retain the chosen depth test but disable depth writes.
- Existing color-attachment synchronization already includes both read and write access.

The factors follow the official [VkBlendFactor specification](https://docs.vulkan.org/refpages/latest/refpages/source/VkBlendFactor.html) and [VkPipelineColorBlendAttachmentState specification](https://docs.vulkan.org/refpages/latest/refpages/source/VkPipelineColorBlendAttachmentState.html).

Forge Sprite now exposes chromaKey: magenta|none and blendMode: opaque|alpha|additive. Defaults magenta/opaque preserve legacy appearance. V2 RGBA imagery explicitly uses none/alpha; skill overlays use none/additive. A new 16-byte push-constant field preserves the existing flip and atlas-UV fields, bringing the Sprite block to128 bytes. Straight texture alpha multiplies Sprite tint alpha. Alpha=0 texels are discarded; blended images retain values below the old 0.02 cutoff.

The ordinary scene and model renderer explicitly choose opaque where legacy behavior is required. The GPU particle experiment selects additive and writes a smooth fragment alpha.

## Reproducible checks

From the RurixForge root:

```powershell
cargo test -p forge-scene
cargo test -p engine-host --bin engine-host viewport::tests
$env:FORGE_BLEND_EVIDENCE_DIR = 'D:/RurixForge/projects/code-sentinels/artifacts/rgba-blend'
cargo test -p engine-host --bin engine-host viewport::tests::sprite_compositing_real_gpu -- --ignored --exact --nocapture
cargo test --manifest-path vendor/rurix/src/rurix-rt/Cargo.toml --target-dir target --lib --features vulkan pipeline_cache_keys_equality
cargo build -p engine-host
```

The explicit GPU test requires real Vulkan hardware and fails if it is unavailable. It uses actual Sprite textures and the actual fixed-function pipelines, rather than a CPU renderer or a shader-only alpha assertion. In one session, opaque, alpha and additive passes share identical shader programs, so a cache that ignores blend mode fails the pixel assertions.

Measured device: **NVIDIA GeForce RTX 5060 Laptop GPU**.

Exact center pixels observed:

- 50% purple over blue: [128, 0, 255, 255].
- Then 50% green additive: [128, 128, 255, 255].
- Half tint opacity: [64, 0, 255, 255].
- Legacy magenta chroma: original blue survives.
- Chroma disabled with opaque mode: purple [255, 0, 255, 255] survives.
- Fully transparent texels preserve the previous framebuffer.

Evidence: projects/code-sentinels/artifacts/rgba-blend/sprite-blend-evidence.json and same-directory PNG/raw RGBA captures. Verified checks: forge-scene16 tests; viewport12 tests plus the separately run real-GPU test; upstream pipeline-key unit test. The GPU particle compute and real viewport tests were also rerun successfully against this patched runtime.

Windows may prevent overwriting a running engine-host.exe. Stop/restart the intended running host through the normal application lifecycle before rebuilding/replacing that executable. The tests use their own hashed executable.

## Renderer frame stability and pack compatibility

Modern 2D Sprite scenes with explicit chromaKey=none, coplanar layers, and additive effects following alpha layers use a fixed pool: 192 ordinary/alpha slots and 64 additive slots. Immutable known textures upload once to device-local memory. Each frame updates resource bindings, UVs, transforms and tint; pooled enemy/effect visibility changes no longer rebuild the entire DeviceFrameSession. Scenes outside this compatibility domain keep the general renderer. The general path also handles a visible blend group exceeding the reserved per-group capacity, so legal entities are not silently discarded.

Real 1280×720 renderer stress test: 60 frames, up to 125 actual Sprite draws, changing enemy/projectile/effect visibility, **0 rebuilds after warmup**, mean 7.69 ms and p95 8.76 ms including readback on RTX 5060 Laptop. The artifact explicitly excludes game simulation and WebSocket delivery; it does not claim whole-game FPS. See pooled-rgba-performance.json and pooled-rgba-frame.png in the evidence directory.

The HostState catch-up loop now releases its mutex after each fixed tick and yields after at most two ticks per outer iteration, preserving the remaining accumulator. This prevents a backlog of 15 expensive logic ticks from monopolizing input/viewport access for half a second.

The existing 16-byte pack push-constant block and shared-texture resource bindings are unchanged. The 128-byte Sprite metadata is local to Sprite raster passes. A separate real-GPU test renders an alpha-blended attachment and runs the exact forge_viewport_pack compute shader with a 256-byte row pitch; all 19×11 RGBA pixels match the attachment bit-for-bit, including composited alpha. See blended-pack-evidence.json. This verifies compute-pack compatibility, not a new D3D12 external-handle lifecycle test.

Reproduce the additional checks with:

```powershell
$env:FORGE_PROJECT_ROOT = 'D:/RurixForge/projects/code-sentinels'
$env:FORGE_BLEND_EVIDENCE_DIR = 'D:/RurixForge/projects/code-sentinels/artifacts/rgba-blend'
cargo test -p engine-host --bin engine-host viewport::tests::pooled_rgba_scene_real_gpu_no_rebuild -- --ignored --exact --nocapture
cargo test -p engine-host --bin engine-host viewport::tests::blended_frame_pack_real_gpu -- --ignored --exact --nocapture
```

## Patch provenance

FORGE_BLEND_PATCH.json records original and patched SHA-256 values. forge-raster-blend.patch contains the manifest and render_exec.rs delta relative to the pinned original package. Other vendored source files are unchanged. This is a focused Forge integration patch, not a claim that the upstream project has adopted the API.
