# Naga 25.0.1 ray-query intersection helper cache correction

Assisted-by: Codex:GPT-6. Local change dated 2026-10-26.

The original crates.io source is copied without editing the global Cargo cache.
`UPSTREAM_PROVENANCE.json` retains its package archive SHA-256, upstream commit,
and every original file digest. The original `.cargo_vcs_info.json` is retained.
The MIT and Apache licenses are copied from that exact upstream commit; their
source URLs and hashes are recorded in the provenance file.

In `src/back/spv/ray.rs`, `write_ray_query_get_intersection_function` accepts
`is_committed`, but the original writer caches only one helper function. A shader
that first asks for a candidate and later for the committed intersection gets
the candidate helper for both calls. Candidate and committed intersection kinds
have different semantics; candidate queries cannot represent a committed miss.
This caused false occlusion in an actual native GPU scene while Vulkan validation
remained silent. Disabling shadows is not a correction.

The only implementation changes are in three SPIR-V backend files:

- `mod.rs`: replace the single optional helper ID with two optional IDs.
- `writer.rs`: initialize and reset both entries.
- `ray.rs`: index the helper cache by `usize::from(is_committed)` on read/write.

No language grammar, material, BRDF, geometry, visibility rule or shader API is
changed. Files carry a prominent change notice. The regression in
`rurix-render::realtime::shaders::tests` checks the generated SPIR-V instructions
contain both candidate and committed selectors. GPU acceptance additionally
checks actual unobstructed illumination, blocked shadows, and light switching.
Execution outcomes remain in the task's separate test receipts; this file does
not claim unexecuted checks have passed.

Upstream source: https://github.com/gfx-rs/wgpu/tree/060a779fc5f56560215ea84bda70278cbbd31e72/naga
