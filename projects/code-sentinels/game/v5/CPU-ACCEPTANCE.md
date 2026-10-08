# V5 CPU acceptance

Run from the project directory:

```powershell
python game/v5/compile_native.py
python game/v5/run_cpu_acceptance.py
```

`--skip-engine` runs only the six actual DLL scenarios. Each scenario starts a
separate process and sets `FORGE_GAME_SAVE_DIR` to a fresh private temporary
directory. The harness loads the exact manifest DLL and uses only `cs5_reset`,
`cs5_input`, `cs5_get`, and `cs5_frame`; no balance, health, enemy, unlock, terrain,
or private state is injected. Native saves still use `code-sentinels-v4.txt`.

The current run passed all six DLL suites and the three targeted Rust commands.
`cpu-acceptance.json` contains the exact DLL, commands and outcomes. Each
`cpu-*-results.json` contains the observed numeric state, with matching `.log`
files. This acceptance covers CPU behavior. It does not claim GPU pixels,
finished-media fidelity, playable scene rendering, or a browser-to-engine
reconnection test. The reconnect invariant here repeats zero-dt ABI publications
and confirms they preserve native state.

Observed results:

- Lifecycle: core frame 0 → 8 at 0.5 seconds → work frame 34 after 2.15 seconds.
  Destruction advances 80 → 126 at 1.95 seconds, holds real frame 127 through
  4.35 seconds, and releases after 4.5 seconds. Pause freezes state and clock.
- Reuse/load: 27 ordinary sell/rebuild cycles saturate all 24 ghost slots. All
  started destruction and wreck holds finish; queued visuals recover. The
  shared alpha FX pool remains capped at 12 and does not also draw additive FX.
- Economy: three purchased wind generators produce 450 power; two GPUs produce
  24 compute/second with capacity 600. Disconnecting their power stops DC work
  at workAge 7.183372; reconnecting resumes the same cycle at 7.266707.
- Research and AI: first research deducts exactly 250 credits and 120 compute.
  A cable's intermediate AI stays untethered, its endpoint AI stays tethered
  even without power, and deleting that cable releases movement.
- Battle: a successful paid attack progresses work frame 32 → 40 in 0.5 seconds.
  Removing compute production stops at frame 40. Actual enemy damage takes a
  turret from 150 to 142 HP with hitTime 0.3; its gameplay slot later clears
  while phase 4/frame 80 remains visible. Repair after an actual core hit spends
  40 credits. Actual defeat retains core destruction and wreck hold while all
  economic fields stay frozen, including pause/resume during the tail.
- Walls: twelve ordinary wall placements close a region around a powered data
  center, charge its shield to 43.2/48, and publish twelve land events. Disabling
  automatic shielding stops charge. Removing one wall publishes type 16 for
  4.5 seconds and opens the region, removing shield capacity.
- Campaign: normal purchases, income waiting, research, upgrades and wave
  commands complete all three levels and all twelve waves. Core HP remains 400.
  The three levels record 149, 201 and 289 attacks; all work frames 32–79 are
  observed. Boss terrain changes affect 21, 22 and 27 cells. Victory freezes
  economic state while the visual clock advances; normal progression unlocks
  levels 2 and 3 in the isolated save.
- Renderer contracts: actual CPU PNG decoding and `.rxsprite` resolution select
  correct atlas family, local frame, bbox and per-frame pivot. Empty variant
  lists preserve old sprite behavior. Invalid stride and family overflow fail
  closed. Changing the selected variant preserves declared texture order and
  object identity. The test textures exist only in the shared temporary test
  project and are not game media or visual acceptance substitutes.

One native defect was reproduced and repaired: an unknown binding kind returned
failure after advancing simulation by 0.25 seconds. V5 now validates the complete
batch before ticking. `cpu-abi-before-fix.json` retains the observed failing
readings; `cpu-abi-results.json` demonstrates unchanged state after rejection.

The Rust test invocation deliberately uses `--bin engine-host`, so it builds and
executes the test binary without replacing an engine executable used by another
running game. Production renderer code is unchanged by the test additions.
