# V6 native pressure checkpoint, 2026-09-11

## Latest result: measured pressure targets passed

The next unified normally executed build SHA9974933A7C984103C6BC09592CE78A6F6BA809CE8E3FC5EF1341220DB88FDFFC passed both requested measurement scopes. `game/v6/pressure-acceptance-20260911.json` and `round4-host-receipt.json` hold the source/executable hashes and limits. Game.step:16267ticks/60.0198s,p50=2.7095ms,p95=9.1878ms,p99=14.3984ms; minimum200moving,all8layers,600projectiles and48488 actual attacks. GPU:2392frames/60.103s=39.798FPS,each layer32.58–44.90,1280x720,no truncation/errors/fallback. No size/range/texture-resolution reduction. The remaining max136.94ms step and204.82ms interframe gap are reported, not hidden. Normal economy/long PvP/authority total advance are separate acceptance tasks. Do not repeat this exact passing fixture without new code changes or a specific unresolved concern.

Last source manifest before/after:162 files,4E4AD33E08F6BFBD215774FF66AC4DA8A3B89405436D5CB4CED998844759D24C. Root/Node may continue ordinary bot/gameplay fixes after this checkpoint, so the evidence applies to the recorded hash. Parent root now owns final release assembly; all engine/pressure processes launched by the subagent have exited.

## Latest continuation (13:40 local and later)

Latest normally executed host is D93FCDB2899F363CF92D4B1C35772ADE8BD5C5C6D5E5AB090C36E36F3FDCC886,20,027,392 bytes. `game/v6/round3-host-receipt.json` records matching162-file final-before/after source manifest FBDE014A658F349B4E8B1AF67DB41DB19BC40140697C589CBAF771774802216A. An initial link was excluded after root added network observation scopes, then the final build was captured and used. All pressure processes have exited.

D93 results:7657 ticks in60.016s,200 units always moving on8 layers,minimum600 projectiles,actual22856 attacks; stepP99=33.6029ms (still fails). The detailed spike is now proven:tick7307 total428.266ms ->cleanup417.968 ->network417.410 ->layout-build382.931ms. Ordinary combat target selection p99=17.2015ms/22.062s accumulated; projectile sweep p99=.9224ms, so do not reduce sweep fidelity or change bucket size without evidence. World event insertion p99=5.7952ms/9.601s accumulated.

GPU D93 now passed every layer in this sample: B2 45.13,B1 41.89,ground35.31,F1 40.92,F2 43.26,F3 40.91,F4 39.12,F5 30.07FPS.2380 real1280x720 frames/60.136s=39.58 average, no truncation/errors/fallback. F5 has little margin, so final verification should keep the same pressure and look at all per-layer buckets. Ground compose median dropped23.0567→3.7649ms, preparation13.094→6.6403ms. `layer-*.ppm` captures are exact RGB frames; bundled dependency Python/Pillow can losslessly convert to PNG for view_image (system Python lacksPIL). Root/source screenshots are not synthetic diagrams.

Newer unbuilt changes: Snapshot.events is now transparent EventLog(VecDeque<Event>) with the same JSON array,4096 cap/order,iter/rev/push/last_mut/extend compatibility. Queue eviction isO(1); gameplay7 tests actually passed including5000 wrap events versus originalVec JSON and fullSave/load,plus ordinary paid opening/replay. Root is optimizing real network topology rebuild (DSU/components and invalidation); Curie is adding spatial target prefilter preserving original order/exactdistance and no-collider fallback. Node is finishing a finite coal stock tail-dispatch deadlock and bot route/research funding issues. Wait for these owners to freeze before next unified link/pressure.

Profiling now exposes `enabled()`, generic `observe(label)` RAII, and `record_substage`; substage values accumulate per tick and are attached to each topSpikes entry, without adding nested values to main totals. Network includes total/layout-check/layout-build scopes; world includes cleanup/events. Full combat subscopes are in ballistics. Terrain caching keeps exact map/fog/viewport inputs and shares groundSprites viaArc; descriptor keys borrow strings and shareArc<Frame>; nothing was downsampled or removed. Fog source memoization also builds visible sets in Pos x/y/z order and adds only the difference to explored; reference12-state transition contract passed again. These new timings are in the current `Logs/v6/pressure/phase-profile.json`.

Additional player rule fixes: extractorBuild requires actual ore/coal with remaining>0 (5-cell range unchanged),with real six-case Build/preview-clone contract passed; worldResearch now calls catalog::research_multiplier, preserving the original values and matching bot/UI cost calculations.

This is an explicitly unearned diagnostic fixture, not an earned economy, LAN combat, or balanced match. Do not reduce the fixture to make results green.

The OS currently permits normal launches of some unsigned files while SAC still reports1. Never relocate/rename blocked binaries or change trust/security policy to force execution. Actual native test programs are selective: state contracts were blocked earlier; after the cache patch, door/equipment test EXEs were blocked by4551, while the new fog reference and trait tests ran normally.

## Last actually executed native builds

- Original accepted normal candidate startup: SHA7A251FFD972B3198C58836D1B940DC7D6CF224E645EFAFBD4E777FF832B8AC12.
- First diagnostics and explicit AI cable bindings: SHA5442671634E24C78E0DE66117D24EE0A9BB56DC9178AABEC48562441259A9059, fixed `game/v6/runtime-bin/engine-host.exe`.
- Phase profiler build: SHA34728260043769F756238E8A72331C7AC1BAC5E535E7E21FDDAED3F15DEE3793, same fixed path. Normal startup and3600 actual ticks succeeded. Cache optimization source changes below are newer and require a new normal link/hash.

The live root UI uses its candidate copy under `dist/CodeSentinels-V6-Windows/bin/engine-host.exe`, so rebuilding runtime-bin does not overwrite that running copy. Coordinate source freeze and heavy-test CPU/GPU windows with root/Node before measuring.

## Fixture and measurement entry

- `game/v6/pressure-probe.mjs`: Node driver, launches native normally, builds a validated Save using the native catalog.128 shell floors (16 footprints times8 levels),512 rooms (DC/lab/factory/depot), actual power/compute wiring and elevators,200 real moving units on all8 layers,600 projectile templates. High HP and continuous ammo/cache/projectile replenishment are deliberate pressure scaffolding.
- Private `game.session.pressureBenchmark` requires `FORGE_V6_DIAGNOSTICS=1` and runs a separate loaded Game, never replaces the player's world and is not present in the LAN bridge whitelist. It warms120 ticks, measures at least3600 ticks and60 wall seconds, keeps minimum600 real projectiles, and records fixture maintenance separately from `Game::step`.
- `crates/engine-host/src/sentinels_v6_pressure.rs` implements the benchmark and emits120 actual native moving/attack/projectile snapshots. The Node driver replays those through native replica `applySnapshot` for GPU measurement, preserving native rendering and local visual animation. This is a rendering fixture, not LAN sync performance.
- All current output: `Logs/v6/pressure/`. `pressure-save.json` validates in native; `render-frames.json` is about168MB; logs are not packaging resources.
- `game.session.metrics` is read-only; bounded8192-sample rings report actual authority Game.step, snapshot clone, publish, and full advance timings separately. Replica metrics only contain publish samples.
- `native-v6/src/profiling.rs` is opt-in thread-local and never serialized. `begin/mark/end` record main phase durations; `record_substage(&'static str, elapsed_ms)` adds nested observations without moving phase boundaries. `phase-profile.json` includes quantiles/totals and12 largest full-tick decompositions.

Commands from D:/RurixForge:

```
cargo rustc -p engine-host --bin engine-host -- -C opt-level=2 -o D:/RurixForge/projects/code-sentinels/game/v6/runtime-bin/engine-host.exe
node projects/code-sentinels/game/v6/pressure-probe.mjs D:/RurixForge/projects/code-sentinels D:/RurixForge/projects/code-sentinels/game/v6/runtime-bin/engine-host.exe --simulation-only
```

Omit `--simulation-only` for the simulation plus GPU suite. Use `--generate-only` to validate the JS fixture through normal native load without invoking diagnostics. The root Cargo profile explicitly compiles the sentinels-v6 dependency with opt-level3; host usesopt2. An earlier standalone `pressure-runner` Rust experiment was not executed: its new serde_json build script was blocked4551. It is not used to bypass anything; the final measurement path is the normally permitted engine's own private diagnostics.

## Actual baseline, not passing

First unprofiled measured3600 ticks in69.925s: p50=13.7623ms, p95=49.7258ms, p99=88.9058ms, max442.296ms. Minimum moving units200; each layer90000 activity samples; actual weapon attacks10720; counts stayed128/512/200 and at least600 projectiles. Fixture maintenance p99=1.709ms excluded from step timing.

The phase-profile run measured3600 ticks in79.326s; step p99=127.764ms. Combat consumed63.515s total, p50=13.4029, p95=36.9871, p99=72.6711, max311.9595ms. Network rebuild consumed8.799s, p99=52.6448ms (every15ticks). Fog consumed1.816s, p99=16.3393ms. Logistics/economy60 invocations had p50=21.148ms and p99=38.968ms. Movement/nav p99=.2612, traits .2079, cargo movement .0691ms were not major hotspots. Target remains Game.step P99<=16.7ms.

First GPU received2230 real1280x720 frames in60.089s, average37.11FPS on RTX5060Laptop, no truncated frames/errors, pre/post meshFallbacks0. Replica publish p99=5.5055ms. **This aggregate does not establish every layer>=30FPS:** warmup ground statuses showed19–22FPS. The updated probe now buckets each layer separately, requires all8 layer averages>=30, and writes actualPPM samples per layer. Rerun after optimizations. Baselines are preserved as `baseline-simulation-report.json` and `baseline-gpu-report.json`; do not misread the older GPU report's aggregate-only passed flag as full acceptance.

## Optimization ownership and current state

- Root: `network.rs`, network contracts. Static components/ports/expanded cells cache. Exact interface `network::NetworkCache` and `network::cache()`, Game.network_topology initialized in new/load.
- Media/Curie: `ballistics.rs`, combat contracts. Exact static collision snapshot and immutable geometry/index cache, dynamic units/shipments refreshed. `CollisionCache:Clone+Default`, Game.collision_cache initialized in new/load.
- Node: logistics source-index optimization frozen, real logistics26/traits8/skills28 tests pass; avoids rescanning empty rooms for every automatic supply request, still verifies live stock and protected buffers.
- Engine agent: fog optimization frozen. Exact terrain/shell/floor/door/wall/shaft topology key, precomputed shaft holes, exact origin/eye/radius source visibility memoization, bounded1M cell indices. No visibility range/frequency reduction. Reference contract actually compared visible+explored over12 successive topology/source changes and passed. Traits8 also pass after this change. `cleanup_dead` early no-op avoids allocations/retains on nonlethal impacts without postponing lethal cleanup; includes stale unit bindings and noncanonical destroyed doors in the cleanup condition. Door/equipment newer EXEs blocked4551; previous versions passed, so don't claim newest tests executed.

All three Game cache fields are already in `lib.rs` and `world.rs` creation/load paths; other agents should not edit these constructors concurrently. Native source freezes are required before next build receipt and pressure run. The next action is wait for root/Curie cache patches and short tests to freeze, normal link, then repeat same3600tick pressure and per-layer GPU measurements. Optimize actual remaining hotspots if thresholds still fail.
