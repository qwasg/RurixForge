# V5 native visual lifecycle contract

The authoritative simulation and command values remain the V4 contract. V5 adds
visual publications, independent subject lifecycles, real frame selection, and
an event ring. The scene action is `cs5`; the exports are `cs5_reset`, `cs5_input`,
`cs5_get`, and native frame ABI v1 entry point `cs5_frame`.
The native frame entry point validates all binding kinds before advancing the
simulation, so rejected batches leave gameplay and visual clocks unchanged.

## Subject frames and physical placement

- Kinds 1 through 10 retain the V4 building kind mapping. Kind 10 walls are drawn
  by the canvas from wall events and wall/shield state, without native wall sprites.
- Kind 11 is the fixed VSCode turret; kind 12 is the fixed PyCharm turret.
  Original AI unit kinds 3 and 4 publish kinds 23 and 24 and keep their existing
  animator assets. Their C5 frame is `-1`.
- Every building/turret variant contains 128 frames. Land uses local frames
  0–31 at 16 fps for 2 seconds. Work uses frames 32–79 at 16 fps. Destroy uses
  frames 80–127 at 24 fps for 2 seconds, then holds the real final wreck frame
  127 for 2.5 seconds before the visual slot is released.
- Phases are 0 absent, 1 land, 2 working, 3 idle, 4 destroy, and 5 wreck hold.
  Idle freezes the last work frame. Work age is preserved across stops and hits;
  returning to work continues that frame sequence instead of restarting it.
- Native frames use `(kind - 1) * 128 + localFrame`. Scene sprites use all twelve
  `spriteVariants`, `variantStride: 128`, and `pixelsPerUnit: 256`.
- The normalized subject body spans 164/256 (`0.640625`) of each frame. Native
  scale is footprint width divided by that span: 3 cells for nuclear, 2 for
  core/DC/hydro/thermal/lab, and 1 for remaining buildings and fixed turrets.
  The origin is centered over the actual footprint. Turret upgrades do not
  enlarge its footprint. AI size/PPU retain their existing behavior.

## Activity and retirement

Power generation, GPU computation, extraction, radio connection, and successful
research drive their respective building work state. A fixed software turret
starts work only after a successful paid attack. Its activity window lasts at
least 1 second and spans its attack interval plus 0.15 seconds, so consecutive
shots continue the work cycle. Starvation, loss of coverage, or interference
stops work; an unsuccessful attack never starts it. Skill casts keep their
separate skill effect and event.

Gameplay death removes the entity from gameplay immediately. Its `LifeTrack`
retains the old position and frame clock through the complete 4.5 second visual
retirement. Reusing a gameplay slot first transfers that old visual into one of
24 ghost slots. If all ghosts are occupied, replacement visuals queue until an
old visual completes or a ghost becomes free. Started destruction and wreck
holds are never truncated. Queued generations have separate stamps, so damage
or retirement of a new generation cannot remove an old generation's animation.

The visual clock and FX keep advancing after the original game phase reaches
victory/defeat. Authoritative economy, enemies, attacks, and movement stay under
the original end-of-game stop. Pause freezes both clocks. Reset/level changes
replace both state sets. Events use the visual clock for their age.

## Publications

All C4 keys, fields, and command numbers are unchanged. C5 adds:

- `C5_Global`: keys 12000–12005 are `[time, eventSeq, ghostCount, fxCount, 5, 0]`.
- `C5_BuildingAnim0..47`: base `13000 + slot * 8`; the six published fields are
  `[phase, localFrame, workAge, phaseAge, kind, hitTime]`. The remaining two keys
  expose visual x/y for diagnostics.
- `C5_UnitAnim0..31`: base `14000 + slot * 8`, same six-field layout.
- Ghost diagnostic keys: base `15000 + slot * 8` for 24 slots, fields
  `[active, kind, localFrame, deathAge, x, y, subjectSlot, owner]`.
- The 256-entry event ring uses base `16000 + slot * 12`. `C5_Event{slot}` reads
  `[seq, type, kind, x, y, age]`; `C5_EventMeta{slot}` reads
  `[duration, subject, owner, magnitude, fxKind, active]`.
- Event types: 1 land, 2 workStart, 3 workStop, 4 buildingHit, 5 buildingDestroy,
  6 unitHit, 7 unitAttack, 8 impact, 9 skill, 10 wallHit, 11 shieldAbsorb,
  12 linkBreak, 13 repair, 14 upgrade, 15 wallLand, 16 wallDestroy, 17 unitDestroy.
- Fixed turrets use unit events, including type 17 for their full retirement.
  Wall events use subject=cell: land lasts 2 seconds, hit lasts 2 seconds, and
  destroy lasts 4.5 seconds. Shield absorption is an independent type 11 event.
  Native wall hit FX are not also emitted. Link break uses its pre-removal
  position and owner, including explicit cable removal.

## Native visual IDs and shared FX budget

- Buildings: 1000–1047; fixed software/AI retain `unitSlot * 4 + unitKind - 1`.
- Ghosts: 1200–1223.
- New additive FX: 1300–1363, restricted to FX kinds other than 3 and 8.
- New alpha FX: 1400–1463, restricted to kinds 3 and 8, maximum 12 active.
- Existing skills: 400–463, restricted to effects with `fx_kind == 0` and
  `overlay == true`. The original 64-effect array is shared by all effect types;
  the native IDs do not allocate additional effects.
- New FX use stride 48. Kinds 1–8 respectively have frame counts
  `[32, 32, 48, 32, 32, 48, 48, 48]` and fps `[32, 24, 24, 32, 24, 24, 24, 24]`.
  Event durations for impacts match the one-shot length. Collapse events also
  cover the subject's wreck hold. FX PPU stays 128; collapse size uses physical
  footprint independently of the subject's transparent-frame normalization.

## Build scope

Run `python game/v5/compile_native.py` from the project directory. It compiles
`Content/Scripts/sentinels_v5.rs` with Rust edition 2021, opt-level 2, panic abort,
and cdylib output. The hash-named DLL and its `rust-cdylib-v1` manifest are written
under `.forge/cache/rxdll`; `game/v5/native-build.json` records the exact result.
The manifest includes source SHA-256 and DLL SHA-256. This operation compiles
only: it does not load the DLL, start the game, or run tests.

CPU acceptance is a separate authorized operation: run
`python game/v5/run_cpu_acceptance.py`. See `CPU-ACCEPTANCE.md` and
`cpu-acceptance.json` for reproduced scenarios, numeric evidence and scope.
