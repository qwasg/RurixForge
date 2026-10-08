# Short actual dual-native integration

This is a150-second integration run, not the final45-minute soak, physical cross-machine routing, balance acceptance, or a new performance benchmark.

Native executable: the normally running fixed `game/v6/runtime-bin/engine-host.exe`, SHA9974933A7C984103C6BC09592CE78A6F6BA809CE8E3FC5EF1341220DB88FDFFC. Both clients launched this same existing path directly; no executable was moved/renamed or substituted to get around SAC. Bridge/controller/server source was frozen for the run. The two clients had separate bridgePIDs4032/19652 and nativePIDs10836/31220. All test processes exited.

Actual result is `lan-runs/short-997493-20260911-b/lan-acceptance.json` (`passed:true`, `finalEligible:false`). Both players issued ordinary paid orders, completed functional buildings, delivered680/670 ore, moved2/1 observed units and fired26/31 actual shots. There were32/31 accepted orders. Native owner binding rejected an enemy-core repair; repeating its sequence returned exactly the original rejected receipt; a command-supplied owner field was rejected byHTTP400. Valid orders were also retried with identical receipts. The proxy delayed every17th request by120ms and interrupted the peer connection for8 seconds; after recovery the measured peer lag was4 ticks and later1–3 ticks.

Both independent native renderers produced actual1280x720 Rurix RGBA streams. Blue changed to layer1 while red remained on ground; captures retained distinct local views. Sampled frame rates were around37–39FPS, but this short test does not claim a separate performance acceptance. The real native save was at tick9219. Loading it created a new two-player lobby and required both players to ready again; both sequence counters[32,32] were restored. Native replay paused correctly, then sought back to tick9219 with owner-one credits45.77750000012005 and accumulated statistics exactly matching the saved state.

Additional quick real native probe (`lan-interface-probe.json`, `lan-owner-probe-20260911.log`) passed actual foreign-building fog filtering, two paid wind-power orders, owner rejection, duplicate receipt,40 advancing peer snapshots,60.23ticks/sec, native save, and real leave-match forfeit. The previous quick-probe receipt was preserved as `lan-interface-probe-before-20260911.json`.

The first run `short-997493-20260911-a` remains a failed result: the old997493 advisory strategy did not build a mobile unit within150seconds. The short driver now explicitly follows a normal paid factory/scout/move plan after a powered connected lab, using the same public preview/order endpoints and genuine construction time. It never injects credits, bypasses tech, advances time, or disables victory. The long-mode strategy remains separate and will use the final native bot/rules build.

## Prepared driver

Run these commands from the game project directory `D:/RurixForge/projects/code-sentinels`.

```
node game/v6/lan-soak.mjs --short --seconds 150 --project D:/RurixForge/projects/code-sentinels --engine D:/RurixForge/projects/code-sentinels/game/v6/runtime-bin/engine-host.exe --out D:/RurixForge/projects/code-sentinels/game/v6/lan-runs/UNIQUE-NEW-DIRECTORY
```

Use a new output directory for each run. Source-project/engine overrides are permitted only with explicitshort mode. Long acceptance remains:

```
node game/v6/lan-soak.mjs --pack D:/RurixForge/projects/code-sentinels/dist/CodeSentinels-V6-Windows --seconds 2700 --out D:/RurixForge/projects/code-sentinels/game/v6/lan-runs/UNIQUE-FINAL-DIRECTORY
```

Wait for the final matched native/UI/media package before running that command. Long mode still requires complete media, both local render streams, at least45 minutes, real combat/resource delivery, the short and45-second interruptions, actual60-second disconnect-forfeit, save/resume, and exact native replay. It cannot gain `finalEligible` fromshort orheadless flags.

After successful runb, the prepared driver additionally gained explicit incoming owner-view assertions (foreign building borders/room centers/units/cable cells, private vision and network grids), ending snapshot captures, delay counters, correctly persisted tick-rate samples, driver/helperSHA, and a replay timeout scaled for a long recording. These additions were syntax-checked. The shared `view-contract.mjs` assertions were then executed against both actual native owner snapshots loaded from the real earned runb save; both passed at tick9221 (`owner-view-native-20260911.json`). The independent actual two-process fog probe also passed. Do not describe these results as having already completed a45-minute run. The final run will record its own exact starting driver/helper and native hashes.
