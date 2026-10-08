# V5 candidate integration runner

`game/test_portable_v5.mjs` runs after real media production, candidate assembly,
and a GPU window are ready. Actual executions retain a separate timestamped
report for each launcher mode; only a report with `passed: true` is acceptance.

Run the two launcher modes separately using the candidate's own Node binary:

```powershell
& '<candidate>\bin\node.exe' game/test_portable_v5.mjs --pack '<candidate>' --off
& '<candidate>\bin\node.exe' game/test_portable_v5.mjs --pack '<candidate>' --on
```

Optional arguments are `--out <new-evidence-directory>`, `--min-fps 30`,
`--fps-seconds 8`, and `--keep-runtime`. The default FPS threshold is 30; the
actual measured value and threshold are both retained, including failure.
The default output is `game/v5/portable-runs/<timestamp>-<mode>`. An existing
evidence directory is rejected to preserve previous results.

The runner refuses a package without the complete 1856-frame provenance
inventory, twelve building atlas variants, or `CommandV5.rxscene`. It copies
only the package's runtime, Content, Web, cache and bridge dependencies into a
fresh temporary directory. Copies are independent files, without hardlinks or
junctions to the candidate. All logs, `.forge/save` and multiplayer journals are
private. The original package and user saves are never altered. Shutdown targets
only the newly launched bridge process tree and its confirmed engine PID.

The script exercises real C4+C5 snapshots and `cs5` commands through HTTP and
WebSocket: initial balances, extractor income, wind generation, data-center GPUs,
power loss and restoration, research, mobile AI, endpoint-only tethering,
paid-attack work frames, and reconnecting without another purchase or reset.
It then samples an active wave, stops compute through ordinary GPU sale, and
records the real core defeat, destruction frames and full wreck hold. Normal
repair commands may protect the core during the bounded FPS window; the sampling
record includes actual enemy count and core HP. It does not repeat the three
full campaign levels already covered by the isolated CPU harness.

Each capture uses actual 1280×720 native RGBA bytes and writes a lossless PNG plus
native frame metadata. Checks reject truncation, mesh fallback, stream errors and
native degradation errors. Images are for visual inspection and contain no
substitute art. HTTP HEAD checks cover every packaged Web file plus literal
bundle references. PVP must report `combatAvailable: false`.

Campaign progress is first checked against the fresh native unlocked value.
After `play_exit`, the actual progress endpoint is exercised against files in
the private save directory, with a report explicitly identifying this as a
file-parser fixture, not progress earned by a simulated campaign. No browser
cache or browser automation is involved. Browser UI interaction remains a
separate root-agent acceptance step.

## Latest verified candidate

The final optimized candidate passed both modes on NVIDIA GeForce RTX 5060
Laptop GPU at 1280×720, keeping the 30 FPS threshold:

- `portable-runs/2026-09-09T07-30-21-319Z-off`: 54.7524 FPS, 459 frames across
  8.3832 seconds; all 16 checks passed and all seven PNG captures were written.
- `portable-runs/2026-09-09T07-32-16-222Z-on`: 54.7177 FPS, 456 frames across
  8.3337 seconds; all 16 checks passed and all seven PNG captures were written.

Both runs observed clean WebSocket closes, no native/stream errors, no truncated
frames or mesh fallback, and removed their private runtime directories after
stopping their own processes. `portable-acceptance-final.json` points to the
full reports and records the tested engine identity. Earlier failing and
intermediate reports remain available for comparison; the final results do not
overwrite them. Browser UI interaction remains separate from these native tests.
