# V6 release evidence contract, schema 2

This document is an input contract, not an acceptance report. Do not create a final approval by filling this document with assumed results. `--no-zip` is always a candidate assembly. Only the completed, independently referenced observations below can authorize the archive branch of `build_portable_v6.py`.

## Identity and file references

A reference is `{ "path": "project-relative/or-absolute-file", "sha256": "64 hex digits" }`. Every reference resolves beneath the explicit `--evidence-root` (default the project). The validator opens and hashes the file; neither a filename nor `passed: true` substitutes for its contents. Synthetic fixtures are forbidden as release evidence. Execution logs and the test runner executable may support local validation but are not shipped. Safe JSON/JSONL summaries, original references and PNG evidence are shipped with their original bytes. `QA/evidence-index.json` maps original paths to packaged paths and records deliberately omitted logs/binaries.

The candidate's `v6-candidate.json.target` contains:

- `engineSha256`: actual `bin/engine-host.exe` SHA256.
- `rulesVersion` and `rulesFingerprint`: compiled native rule identity, subsequently verified by an actual `game.session.catalog` result.
- `payloadSha256`: SHA256 of runtime file records, sorted by path. Each record is `{path,bytes,sha256}`. Serialize the record array using Python `json.dumps(records, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode('utf-8')`. Runtime files are `bin/**`, `Content/**`, `Web/**`, `v6/**`, `bridge.mjs`, `multiplayer-v6.mjs`, `forge.toml`, `Start-Game.cmd`, `Start-Game-GPU-Particles.cmd`. QA added after testing does not change this digest; changing any runtime, asset or web file does.
- `webManifestSha256`: the same record-array hash restricted to `Web/**`.
- `mediaManifestSha256`: SHA256 of `Content/UI/v6/resource-manifest.json`.

All hashes are compared case-insensitively and stored lowercase. Candidate markers retain the identity fields at top level for older read-only tools, alongside the identical `target`. `files` and `included` are frozen only after all final QA copies. The marker's own path is included but its own recursive hash is explicitly excluded from `files`. All other files are individually hashed. The archive verifies exact names, CRC, individual file hashes and the final marker again before its final filename is published.

## Final approval envelope

Required top-level fields are `schemaVersion: 2`, `version: 6`, `status: "approved"`, `target` exactly matching the candidate target, and `evidence`. The evidence object has seven separate file references: `hostIdentity`, `nativeBuild`, `functionality`, `balance`, `performance`, `lan`, `coldStart`. This is a manually reviewed aggregate of actual completed observations; the packager never creates it.

Except the runner-specific balance section and build receipt, evidence documents carry `engineSha256`, `rulesVersion`, `rulesFingerprint`. Performance may use its existing `native.sha256` field instead of `engineSha256`. LAN and cold start also carry `payloadSha256`. Historical media QA records its own tested engine hash explicitly; it is never relabelled as having run on a newer host.

## Native host and build

`hostIdentity`: `kind: "native-host-identity"`, `actualNativeExecution: true`, positive `nativePid`, `observedMethods` including `game.session.catalog`, and `catalog` referencing the actual raw catalogue object. The catalogue must report `version: 6`, the same rules version and fingerprint.

`nativeBuild`: actual build receipt with `buildExit: 0`, `artifact.sha256`, rules identity, and `source: {unchangedDuringBuild: true, beforeManifest, beforeSha256, afterManifest, afterSha256, fileCount}`. Both source manifests must be byte-identical arrays of `{path,sha256,bytes}`. Paths resolve from the repository. Every current source must still match; the inventory must include workspace Cargo files, both native game and engine-host Cargo/build files, and their complete Rust source trees. A later source edit invalidates this receipt for final publication.

## Functionality

`kind: "native-functional-acceptance"`, `finalEligible: true`, identity, and these independently referenced sections:

- `nativeTests`: `actualNativeExecution: true`, positive `passedTests`, `failedTests: 0`, `unexecutedTests: 0`, nonempty `evidence` reference array. `coverage` includes `construction-and-layers`, `weapons-and-four-defenses`, `power-and-compute-isolation`, `ground-and-air-logistics`, `research-and-plugins`, `ownership-and-idempotency`, `split-merge-and-collapse`, `save-load-replay-identity`.
- `ordinaryOpening`: `actualNativeExecution: true`, `ordinaryPaidCommands: true`, `injectedResources: false`, `startingCredits: 2000`, `creditsRemaining >= 500`, `acceptedOrders >= 10`, and nonempty `evidence`. `built` records actual extractor >= 1, power >= 1, dataCenter >= 1, gpu >= 1, lab >= 1, starterTurrets >= 2, powerLines >= 1, computeLines >= 1.
- `nativeUi`: `actualNativeExecution: true`, positive `rgbaFrames`, `errors: []`, nonempty `evidence`. `completedActions` includes `new-solo`, `build-room`, `power-wire`, `compute-wire`, `install-gpu`, `deploy-unit`, `skill-target`, `change-layer`, `save-load`, `replay`.

An earlier blocked test invocation is preserved as historical evidence. A later actual passing invocation may resolve that test; no unexecuted invocation may be counted as passing. Visual replica fixtures cover rendering only and cannot stand in for the ordinary paid opening or gameplay tests.

## Balance and strategy

`kind: "native-balance-acceptance"`, `actualNativeMatches: true`, `injectedResources: false`. `runner` contains `artifact` reference, `rulesVersion`, `rulesFingerprint`. The artifact is the actual runner executable and must not be labelled `engineSha256`. Its compiled rules and every receipt's `simulationSourceSha256` must equal the separately observed final native host fingerprint.

`branchMatrix: {inputs: [JSONL references], analysis: reference}` must contain exactly 1,000 unique canonical cases indexed 0–999. Branch order is speed, security, algorithm, science, lightweight. For index `i`: original pair is `[branches[i//200], branches[(i//40)%5]]`; seed is `1000 + (i//2)%20`; theme is river/mining/highland cyclically by seed offset; swap is odd index. Swap reverses both branches and planner order; planner is `[1,2]` on even cases and `[2,1]` on odd cases. Both strategies are `mixed-ai`. This explicitly measures the branch and planning-order factors; it yields 1,000 distinct scenario configurations. Historical pilots missing planner order are not accepted as final cases.

`strategies: {plan: reference, inputs: [JSONL references], analysis: reference}` must match `strategy-plan.json` (`planId: v6-strategy-126-v1`) exactly: six strategies, 21 unordered pairs including self comparisons, three theme/seed pairs, two side swaps = 126 cases. Both players use the same branch selected by `(pairOrdinal + themeOrdinal) % 5`. Cross-strategy swaps keep planner `[1,2]` so each strategy occupies the first-planning blue side once. Only swapped self comparisons use `[2,1]`. Each receipt binds `planId`, `planIndex` and SHA256 of the complete plan file. All 126 configurations are distinct. Self comparisons are controls, not evidence of superiority between strategies.

Rows preserve null first/second T5 times; null is never zero. They require real nonnegative two-player ore, ammunition, compute, energy, shots, losses, node control and recoverable cargo metrics; explicit planner order; actual elapsed time; and five boolean playability warnings. Unresolved no-combat, no-damage or no-ore-delivery observations reject final acceptance. Both input sets are separately checked against their analysis input hashes, counts, fingerprints, distinct scenarios and actual duration median. `aggregatorSha256` must match the reviewed current `summarize_balance.py`; the gate recomputes every summary statistic from the exact ordered inputs and rejects altered faction/timing/exchange results. The summarizer's `finalBalanceAcceptance` must remain false: statistical integrity alone is not balance approval.

`decision: {status: "accepted", blockingIssues: [], criteria: {...}}` contains six separately reviewed entries: `branch-matchups`, `strategy-comparisons`, `match-duration`, `technology-timing`, `mixed-ai-and-hard-kill`, `resource-control-and-logistics`. Each has `outcome: "accepted"` and a concrete rationale, at least 12 characters. This gate does not invent a win-rate cutoff or approve incomplete runs. A pilot cannot substitute for either full matrix.

## Sustained performance

The actual pressure report must match the final engine and rules. `simulation` requires at least 128 shells, 512 rooms, 200 units, 600 minimum pre-step projectiles, 200 minimum moving units, 3,600 samples/ticks, 60 real wall seconds, positive actual weapon attacks and samples on all eight layers (-2 through 5). `stepMs.p99 <= 16.7` milliseconds.

`gpu` requires at least 60 wall seconds, 1,800 frames, observed >= 30 FPS, and all eight `perLayer` entries each with >= 7 seconds, positive frame count and >= 30 FPS. `errors: []`, `truncatedFrames: 0`; at least two actual 1280×720 `frameDiagnostics` observations must show `meshFallbacks: 0`, `truncated: false`, and a named device. Existing maximum tick/frame gaps are retained in the derived QA; passing P99 does not mean zero stutter.

## Two-client LAN endurance

Report identity includes package payload. Required: `finalEligible: true`, `short: false`, `requestedSeconds >= 2700`, `observedCombatWallSeconds >= 2700`; exactly two `clients` with different positive `enginePid`. Each client has matching engine/rules/payload identity, `rgbaLocal: true`, positive `frames`, positive `accepted`, positive `movedUnits`, and named `device`. Both entries of `allCombat` and `allOre` must be positive. Two browser windows sharing one native process do not qualify.

`coverage` includes `ownership`, `deduplication`, `delayed-transport`, `disconnect-recovery`, `timeout-forfeit`, `independent-camera-layers`, `save-load`, `exact-replay`, `consistent-outcome`. `consistency` records a positive aligned `tick` and equal SHA256 fields `authorityOwnerViewSha256` and `replicaSha256`. `errors: []`, `replayExact: true`. Preserve factual subreports supporting these observations; the final boolean alone is insufficient. A 150-second short test, even if it says passed, cannot meet this gate.

## Isolated cold start

`kind: "isolated-cold-start"`, matching engine/rules/payload, `actualNativeExecution: true`. `isolatedDirectory` is an absolute directory outside both the project and assembled candidate, preferably with Chinese characters and spaces. Initial state must be `initialRuntimeFiles: {saves: 0, tokens: 0, logs: 0}`.

Required facts: `usedBundledRuntime: true`, `requiresCodex: false`, `externalCodeServicesUsed: []`; `completedFlows` includes `solo`, `create`, `join`, `save`, `load`, `replay`; positive `nativeRgbaFrames`, `errors: []`, `finalEligible: true`. `processes` contains distinct positive process IDs and absolute `executable` paths inside the isolated directory, including both `node.exe` and `engine-host.exe`. Existing Codex installation on the test computer is not evidence of a game dependency or of an uninstall test. Inspect actual process paths and network/service usage instead.

## Candidate preservation and final distribution

The output basename is `CodeSentinels-V6-Windows`, separate from V5. Existing directories require explicit `--refresh-candidate` and a V6 candidate marker; final or unknown outputs are refused. Refresh moves managed old files into `.v6-candidate-history/<timestamp-id>/...` and retains runtime `.forge`, `Logs`, saves and unrecognized user files. It never recursively deletes V5, previous candidates, old archives or source receipts.

Candidate assembly does not require final approval and cannot create a final ZIP. Final archive creation requires the exact approval above, checked before staging and again after file copying. Evidence changes, runtime hash changes, stale Web mirrors, missing author references, secret-bearing JSON, unexpected managed files or an existing final ZIP all stop publication. A uniquely named build receipt is retained outside the output. Final archives use only the marker's exact allowlist; local logs, credentials, environment files, development caches, saves and the balance runner binary are excluded.
