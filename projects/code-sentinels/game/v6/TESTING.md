# V6 verification workflow and evidence boundaries

The approved requirements are in `../../V6-APPROVED-PLAN.md`. No candidate is a final release merely because it builds or shows a frame.

## Evidence already obtained

- `protocol-tests.json`: ten real Node HTTP/SSE boundary groups using an explicit authority adapter double. Includes owner binding, required room code, anonymous-info privacy, expired lobby-seat recovery, ordered retries, rules hash, preview, deltas and 60-second authority-forfeit dispatch. Public/private IPv4/IPv6 literal parsing is tested without connecting to external addresses. It is not native combat evidence.
- `controller-tests.json`: seven local session lifecycle groups using an explicit RPC double. Includes unique solo/load epochs, local picking and ownership, pause, stale requests, delayed receipts, replay controls and PVP save restoration through a new ready room.
- `reconnect-bootstrap-tests.json`: three localhost HTTP/SSE recovery checks with explicit native RPC doubles. An initial snapshot HTTP failure, initial snapshot-apply failure and silent initial stream recover without reopening the replica; advancing snapshots resume. This does not launch native code or prove real combat synchronization.
- `collection-delta-tests.json`: eight synthetic protocol 6.1 entity-delta checks. The measured example payload reduction is a fixture result, not a network throughput or native FPS result. This report is included in candidate QA.
- `lan-interface-probe.json`: two actual independent native processes, real local bridges and LAN HTTP/SSE. Both players built through ordinary commands; native rejected foreign repair; duplicate orders were idempotent; remote fog was filtered; full save and native forfeit worked. Forty distinct received snapshots measured approximately 59.65 native ticks/s. This is a short integration probe on engine SHA256 `2659ac9bca55351afb6d40cbff4c4cfd9fad8af009c7e2ede89306f38681782a`, before subsequent gameplay changes. It is not 45-minute combat, final media or pressure acceptance.
- Six catalog tests and an earlier two-test bot round executed before subsequent changes. The early 600-second bot runs revealed stalled cargo and no mobile army. Bot spacing, dynamic logistics and stronger assertions were added afterward; those changes need execution on the final native build.

## Historical system execution block — 2026-09-10

The 2026-09-10 revision includes 28 isolated skill contracts, 20 isolated logistics contracts, and eight trait contracts. Logistics fixtures deliberately advance only the logistics clock with declared stock/power; they do not represent earned campaign progress. Trait checks cover real component/floor/range gating, non-stacking bonuses, paid Claude/GPT passives, invalid-target/no-funds cases, material-versus-AI-healing separation, and status-duration preservation. Unloading assertions now wait for amount/40 seconds and verify that boosts shorten time without adding cargo; old immediate-delivery assumptions were removed. `cargo check --tests` and a complete `cargo test --no-run` succeeded after integration. None of these new native assertions has executed while Smart App Control remains active. `native-static-checks.json` and `native-build-receipt.json` record compilation/linking only; Node protocol and controller results remain separate from native playability.

Three additional `v6_bot_progression` contracts cover a paid relay connected by actual bot orders, rejection of starting-tier airport/launch-pad fabrication, and five separate ordinary-command bot runs toward combined ground/AA/air/orbital production. Each branch run observes at most 45 simulated minutes and honors the normal winner; failure to reach a required class remains a failure. These tests do not inject credits, technology or airports. They have only compiled and linked; the five-branch progression results are pending native execution. The updated balance-runner also passes `cargo check` without running its executable.

On 2026-09-10, new native test and engine binaries were refused by Windows Smart App Control. Test startup returned WinError 4551. The UI engine spawn returned UNKNOWN; the Code Integrity Operational log recorded Events 3077/3033 at 13:15:48 for `game/v6/ui-runtime/engine-host.exe`, signing policy ID `0283ac0f-fff1-49ae-ada1-8a933130cad6`. The root agent independently identified Smart App Control and asked the user about a supported resolution.

No policy was disabled, binary relocated to evade the policy, or blocked result recorded as a pass. Compilation and JavaScript protocol checks remain available. Resume native execution only once the user has resolved the system policy or supplied compliant signing. The test UI instance was closed; existing V5 games were preserved.

## Commands after the system permits execution

```powershell
node projects/code-sentinels/game/v6/test_protocol.mjs
node projects/code-sentinels/game/v6/test_controller.mjs
cargo test --manifest-path projects/code-sentinels/native-v6/Cargo.toml
cargo build --release --manifest-path projects/code-sentinels/game/v6/balance-runner/Cargo.toml --target-dir projects/code-sentinels/game/v6/balance-target
```

First run six policy smoke cases, then inspect real mining delivery, mobile armies, first combat, research progression and unresolved matches:

```powershell
projects/code-sentinels/game/v6/balance-target/release/sentinels-v6-balance-runner.exe --out smoke-new.jsonl --seeds 1 --smoke true --limit 6 --minutes 55
```

The runner never awards a winner at its observation limit; unresolved games stay unresolved in its report. It records first combat, first T1–T5 and second T5, ore deliveries, ammo consumed from fire events, core impact damage, cross-floor movements, node control, losses, accepted/rejected orders and peaks. Empty-army or no-combat draws are playability failures, not evidence of balance.

After correcting smoke failures, the default branch matrix uses 5×5 branch combinations, 20 seeds and swapped sides (1000 enumerated runs). `--first` and `--limit` permit independent CPU shards with separate new output files. `--strategy-matrix true` additionally enumerates expansion, main technology, multiple technology, mechanical, mixed-AI and turtle policies. Run CPU shards moderately; keep the GPU available for real media generation until it finishes.

The mandatory 45-minute two-client combat, delay/reconnect trials, complete replay, all-floor pressure, final renderer performance and isolated package cold start remain separate acceptance steps. The short LAN probe must never substitute for them.

`lan-soak.mjs` is the prepared real-time driver. It requires a separately assembled candidate and, for rendering, completed media. It copies two independent runtimes, obtains read-only native bot suggestions and submits them through each client's ordinary authenticated command route. The default is 2700 seconds with two local Rurix streams, repeated commands, bounded artificial latency and temporary disconnects. `--headless` or shorter observations are explicitly ineligible as final render acceptance. This driver has only received syntax checking while Smart App Control blocks the new binaries.

## Actual execution resumed — 2026-09-11

Normal execution at the original paths became possible for the existing candidate engine and subsequent normal Cargo builds. Smart App Control remained enabled; no policy, executable name or executable location was changed to bypass it. The initial skill executable rejection is retained in `skills-executed-20260911.log`; the updated skill binary later ran normally at the same path.

Actual owned subsets now include 28 skill contracts (`skills-executed-updated-20260911.log`), 26 logistics contracts (`logistics-source-index-20260911b.log`), and eight trait contracts (`traits-executed-20260911.log`), all passing. The initial library run was 52 passed/one failed; its factory-stock assertion incorrectly expected a 120-unit target to arrive in the first courier despite the 100-unit limit. The corrected test waits for real successive deliveries and checks owner-specific stock conservation. Three additional focused bot checks cover vehicle-width alleys, occupied footprint reservations and rejecting wreck-only mine sites. These subset results are not a claim that the full release suite passed.

The first six-policy native smoke found five games without any combat. Real saved-state inspection traced this to mechanical units being incorrectly tethered at compute endpoints and facilities repeatedly exporting each other's operating stock. After those fixes, `smoke-current-20260911.jsonl` records six actual games with combat, but revealed one-cell building alleys that blocked later 2×2 vehicles. The read-only `--inspect` runner loads the real save and uses native `route`/`can_step` without editing state; its evidence is `diagnostic-current-20260911/match-1.navigation.json`.

After reserving two-cell alleys, the expansion mirror in `smoke-alley-20260911.jsonl` lasted 2579 seconds with 22186/22352 delivered ore and 1882/1632 paid attack rounds. Other policies still exposed development defects: wreck cargo was treated as an extractor site, exhausted mines counted against expansion, and budgets ignored the existing extra-branch research multiplier. Subsequent repairs preserve actual costs and victory rules; the full post-repair strategy smoke and required 1000-game matrix remain pending. An intermediate run launched after a failed build is explicitly invalidated by `smoke-fixes-maintech-20260911.invalid.json` and must not be used as repair evidence.

The real engine pressure fixture measured logistics total P99 at 0.5169 ms after indexing current supply sources, versus approximately 38.97 ms before. Overall simulation/GPU performance and the 45-minute two-client LAN trial have separate owners and acceptance records. `execution-status-20260911.json` tracks these staged results without declaring release acceptance.

Later on September 11, all 28 logistics tests passed after adding real full/exhausted-mine tail deliveries (`logistics-all-tail-executed-20260911.log`): fuel is retained, the normal courier fee is paid, and fragments worth less than the fee are not automatically shipped at a loss. All six current catalog tests passed. The actual map audit passed 60 generated maps (three themes × 20 seeds), including equal opening construction area/nearest-ore distance and bounded objective-path differences. Later six-policy smoke rounds are retained as `smoke-renewal-20260911.jsonl` and `smoke-army-20260911.jsonl`; both have actual combat, but mixed-army and high-tier usage still need improvement before the matrix.

The first actual `v6_bot_progression` execution passed two contracts and failed the five-branch orbital contract at its first branch: speed earned T5 and actual ground/AA/aircraft, but normal node victory occurred before the orbital purchase. That failure remains recorded, not weakened into a pass. A subsequent diagnostic-only test rebuild at the same original executable path was independently denied by OS application control with error 4551 and never executed (`bot-progression-diagnostic-executed-20260911.log`). No alternative executable was used to bypass that denial.

After genuine bot power-planning and strategic-purchase repairs, a normal rebuild at the same original path executed successfully. The latest production run passed all three contracts in 35.38 seconds (`bot-progression-strategic-budget-executed-20260911.log`), including all five separate branch scenarios purchasing ground, AA, aircraft and orbital units before the normal game ended. Each starts with the real 2000 credits and uses ordinary paid commands; the five final saves are retained in `bot-progression-strategic-budget-20260911/`. No purchase price, initial resource allocation or victory rule was changed to obtain the result. The isolated historical OS rejection remains a rejection, not a retroactive pass.

## Portable package

`build_portable_v6.py --no-zip` creates a separate V6 candidate from verified retained assets, new media roots, current Web output, bundled Node/CRT and the new engine. Refreshing a marked candidate requires `--refresh-candidate`; no recursive deletion or V5 replacement occurs. Final archive creation requires complete character media plus a successful `game/v6/acceptance.json` whose engine hash matches the packaged binary. Runtime logs, tokens, saves and stale Web chunks are not included.

## Rules identity and balance pilots - September 11

Native Save/load/replay now require the exact compiled rulesVersion and rulesFingerprint. The local controller obtains this identity from its own engine catalog, validates both the wrapper and inner Save, and lists unsigned old development saves as incompatible without deleting or rewriting them. Seven controller groups and six save-identity groups passed using explicitly labelled RPC doubles; native replay has separate actual evidence.

Pilot 1 stopped after 53 complete zero-combat rows. Its red factory-centre spawn test was wrong for a 2x2 vehicle beside the shell boundary. The real planner correction uses all valid factory anchors. A normal 2000-credit mirrored opening subsequently moved each whole vehicle outside at 109.3 seconds. Failed rows and the stop receipt are retained.

Pilot 2 completed 100 rows with actual combat and deliveries, but only 50 distinct seed/theme/branch/strategy configurations because every planner used owner 1 first. Mean duration was 1500.86 seconds; none reached 30 minutes. Blue won 62; algorithm won 80 percent of 40 observations and science 15 percent. Six player observations reached T5. These results fail balance acceptance. Two actual terminal states showed all data centres and labs per side in the same compute component; their late research was a spending-policy problem, not a disconnected GPU bank.

Pilot 3 runs four frozen 25-case shards with explicit plannerOrder and local-store budgets. It adds early T2/AI saving, grouped ground orders, two node garrisons plus surplus core scouting, and science scout price126/speed3.0 only. Nine tactical contracts actually passed. The new factory test and plan parser test executable were separately rejected with OS4551, so are not passes. The normal runner product ran at its existing path. The checkpoint pilot3-policy-checkpoint-20260911.json records its fingerprint and binary hash. Initial T2 times improved to174 seconds and peak AI reached2, but repeated elite replacement spending and unresolved55-minute games remain real failures.

Final strategy comparison is an explicit126-case plan:21 unordered pairs (including self-play) of six policies, three themes/seeds and swapped sides. The plan schema is {schemaVersion:1,planId,cases:[{index,seed,theme,branches,strategies,swapped,plannerOrder}]}; rows include the exact input-file SHA and case index. Cross-strategy pairs swap strategies but keep plannerOrder[1,2]; self-play's swapped case uses[2,1]. The1000 branch factorial has all ordered5x5 pairs x20seeds x2swaps; swapping also reverses planner order. Old rows are never relabelled. The older exhaustive --strategy-matrix remains a diagnostic option, not the final126-case plan.

The publication gate must separately bind functionality, balance, performance,45-minute LAN and cold-start evidence. Balance uses compiled rule identity to match the final host; CPU runner and engine binary hashes remain distinct. None of these pilots is final release acceptance.

The fourth pilot completed100 genuine cases: mean2297.49 seconds, blue46/red32/22 unresolved,88 player observations reaching T5, minimum1651 and median2275 seconds. None reached the18-22-minute first-T5 target; this remains a failed timing/deadlock diagnostic despite improved higher-tier usage. See matrix-pilot4-analysis-20260911.json and its immutable source/binary checkpoint.

The fifth bounded repair gives an ordinary T1 scout a reconnaissance role, lets the first AI support the field, funds missing income/science prerequisites, and prepares actual power headroom before extra equipment. Basic facilities use their real construction price plus80 working capital. Its normal2000-credit opening test actually bought a T1 scout at69.0 seconds and discovered forward ore at82.5 seconds before a second AI purchase. Both mirrored whole-vehicle exits occurred at70.3 seconds. Thirteen focused tactics/power contracts ran successfully. These opening observations do not yet establish a complete balanced match; the new product has compiled and awaits the next pilot after the independent LAN development window.
