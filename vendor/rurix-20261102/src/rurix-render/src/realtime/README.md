# Scene-driven real-time Vulkan rendering

The opt-in `realtime` feature owns the native GPU pipeline used by RurixForge.
It consumes `SceneSnapshot`, typed incremental updates and explicit frame/output
requests. It does not launch Bistro/HQ benchmark binaries or substitute proxy
geometry. RXS-0408 through RXS-0411 describe the interface contract.

Opaque and masked primitives share a persistent vertex-reference stream and one
hardware raster pass. The optional depth prepass uses that same stream, jitter,
sidedness and alpha test; it is disabled by default pending target-scene A/B.
RayQuery consumes the matching BLAS/TLAS, instance transforms, material table and
authored lights. Color textures retain their authored RGBA8 bytes and all mips;
an immutable CPU-decoded sRGB table supplies linear f32 values before filtering.
Normal/data mips are treated separately. No texture is silently resized or lost.

Indirect diffuse and glossy reflection have separate persistent histories and
variance-guided spatial filters. Camera cuts, actual surface invalidation and
abrupt lighting edits reject stale history. Smooth light movement raises the
current-frame weight instead of clearing every frame. Transparent output writes
its own motion/reactive mask; marked history cannot leak into newly revealed
opaque pixels. This is a thin alpha layer, not multiple-interface refraction.

Eight-by-eight GPU light lists are conservative world-space bounds candidates;
the exact range/cone/BRDF/visibility tests remain in shading. `tile_lights=false`
is the exhaustive comparison path. Depth and light-list optimizations do not
change output resolution or silently remove authored lights.

Native output imports two caller-owned linear D3D12 buffers. The caller must
finish both external consumers before reuse, resize, detach or destruction.
The runtime performs actual external ownership acquire/release and checks the
adapter LUID. A missing native slot is an error. Explicit headless benchmark
intent can omit output, and readback occurs only when requested. Allocation
ledger totals and sampled driver heap usage are distinct; neither is a measured
continuous VRAM peak.

## Compiler corrections and evidence boundary

The vendored Naga 25.0.1 patch separates candidate and committed ray-intersection
helper caches. Its upstream provenance and three-file diff are documented in
`vendor/naga-25.0.1/PATCH.md`. The regression compiles both first-call orders and
requires both SPIR-V intersection selectors.

Naga 25 also decorates workgroup-only arrays with `ArrayStride`, rejected by the
installed Vulkan validator as VUID-StandaloneSpirv-None-10684. `shaders.rs` performs
a restricted structural normalization: it traverses variable pointees and their
type graphs, removes only `ArrayStride` from exclusively Workgroup-reachable
arrays, and rejects any such array also reachable from another storage class.
Storage/Uniform/IO layouts are preserved. No instruction IDs, arithmetic, bounds,
control flow, or external ABI are changed. Tests cover exclusive and shared-type
cases; GPU tests require actual validation success. With the test-only
`RURIX_REALTIME_SHADER_AUDIT_DIR` set, before/after SPIR-V and the exact WGSL are
retained for independent inspection. Production builds do not write these files.

Functionality, source compilation, visual quality and target frame rate are
separate acceptance results. Unit tests alone do not establish an AAA visual
result or engine-wide Godot/UE equivalence. Target-scene captures, continuous
motion, actual native presentation and performance measurements remain required.
