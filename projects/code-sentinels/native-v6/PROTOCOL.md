# Code Sentinels V6 native protocol

All methods return ordinary engine JSON-RPC `result`, never MCP content wrappers. Owners are 1 and 2; neutral is 0. The transport derives owner from its authenticated session. World coordinates use integer `x:0..127,y:0..95,level:-2..5`. JSON fields are camelCase; command `type` and content IDs are kebab-case.

- `game.session.open {mode:"authority"|"replica", seed:1, localPlayer:1, opponent:"human"|"ai"}` resets to a fresh game and returns `{snapshot}`. Replica does not advance economic/combat simulation.
- `game.session.order {owner:1, sequence:1, command:{type:...}}` returns `{accepted,sequence,tick,reason}`. Per-owner sequences are monotonic and identical retries are idempotent. Rejected gameplay commands also consume a sequence.
- `game.session.snapshot {owner:1}` returns the V6Snapshot directly, with `version:6,revision,tick,seed,width:128,height:96,minLevel:-2,maxLevel:5,players,terrain,excavated,buildings,rooms,entrances,units,links,walls,resources,shipments,projectiles,events,winner,winReason`. Owner-filtered snapshots hide currently unseen enemy entities. Omitting owner is private authority save/debug only.
- `game.session.applySnapshot {snapshot}` atomically applies a whole V6Snapshot to a replica; same tick/revision is an idempotent no-op; older snapshot is rejected. Returns `{applied,tick,revision}`. Client render state never runs an authority simulation.
- `game.session.view {x:64,y:48,level:0,zoom:1,localPlayer:1}` changes only this machine's native camera/render selection. No match command is generated.
- `game.session.catalog {}` returns costs and metadata matching the native rules.
- `game.session.save {}` returns full serializable state + command replay. `game.session.load {save}` restores it; `game.session.replay {save}` deterministically checks replay from initial seed to saved tick. These are local owner controls, never LAN public methods.
- `game.session.close {}` closes the session and returns `{closed:true}`.

Authoritative ticking is 60 Hz in engine-host. Transport publishes snapshots at 20 Hz; no viewport pixel bytes cross LAN. V5 APIs remain available separately.

Commands and Rust source-of-truth types are in `src/types.rs`. The initial state and catalog can also be produced without the renderer by `cargo run --example snapshot`.
