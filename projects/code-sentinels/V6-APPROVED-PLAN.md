# V6 approved implementation contract

User explicitly approved this implementation on 2026-09-10 and approved the **flattening simplification** on 2026-09-18 (single ground level, abstract supply). This file records the approved behavior for independently running agents. These are required features, not optional stretch goals. Preserve V5 entry/save/archive. Report actual evidence separately from remaining work; never label stubs, mock state or generated static stand-ins as final.

## 2026-09-18 simplification (supersedes conflicting lines below)

- Goal: building a base only requires thinking about **space** (footprint / net area) and **resource utilization** (credits, power, compute). Floors, vertical structure and physical transport were removed from the design.
- **Single ground level.** Logical `z` is always 0. No underground floors, no upper floors, no excavation (soil/rock/aquifer), no stairs/elevators/ramps/shafts, no corridors/columns, no structural support ratio or collapse cascade, no per-floor fog, no floor tech caps, no cutaway/layer camera. Serialized `z` fields remain for wire compatibility and must be 0.
- **Buildings are solid footprints.** Shells and outdoor buildings block ground and vehicle movement; aircraft fly over everything. Units never enter buildings, so there are no doors or entrances. Factories spawn vehicles on a free 2x2 cell ring immediately outside the factory shell; AI deploy to any walkable ground cell within 12 cells of the producing room.
- **Capacity = net area.** Net usable area equals the room's gross area (no corridor/shaft deductions). Capacity is `floor(netArea * catalog capacityPerArea)`; `capacityBudget/potentialCapacity`, split/merge conservation and the room HP formula `140*(area/4)^0.85` are unchanged. Shell cost no longer has a height factor.
- **Abstract supply.** Ore is finite; a completed extractor credits its owner **directly** each second (`totals["ore-mined"]`). There are no shipments, depots, ammunition workshops, cargo stock, ammo/fuel deliveries, ambush/wreck salvage or supply/reroute commands. Units have no ammo; kinetic weapons fire whenever cooldown allows. Ground vehicles have no fuel. Aircraft keep an abstract **endurance** (`fuel/fuelMax`): it drains while airborne, the aircraft returns automatically below 20%, and a powered airstrip restores it while landed (no cargo).
- **Repair without materials.** `repair(id)` costs credits and still needs a construction drone route. `repair-bay` is a powered/online passive aura healing friendly units within 12 cells. Coal/nuclear plants no longer burn delivered fuel; they pay a per-second credit upkeep and stop when the owner has no credits. Speed's `rapid-logistics` becomes a rapid-readiness hub (faster aircraft readiness and repair aura within 12 cells). `field-resupply` becomes `field-recharge` (restores battery/energy).
- **Kept unchanged:** power/compute networks and isolation by connected component, GPUs, research branches and labs, AI roster/skills/plugins, mechanical tiers, energy/battery consumption, four defenses (CUDA regions now computed on the single ground layer using walls only), strategic nodes and victory rules, deterministic saves/replays, LAN host authority.
- **Acceptance:** all pre-2026-09-18 balance, pressure and LAN evidence is historical for its own fingerprint. New pressure fixture is single-layer 128 shells / 512 rooms / 200 units / 600 projectiles. Balance matrix, 45-minute LAN and portable release must be rerun on the flattened rules before any approval.

## Match and world

- Complete single-player versus a resource-paying economic opponent AND real two-client LAN/direct-IP base combat. Standard target 30–45 minutes. Opponent uses same costs, vision, income and movement as player. No waiting for manual wave trigger/infinite risk-free hoarding.
- 128x96 microcell map, river/mining/highland themes, deterministic seeds, fair starting resources and reachable central objectives. Owners 1/2, neutral0; independent fog affected by structures, terrain and walls.
- Single ground level z=0. Every structure is a solid footprint; aircraft fly over structures using `altitude`, never a floor index. Orbital hits resolve against ground structures with the ordinary defense sequence.
- Isometric 2:1 projection, zoom/pan; native rendering and UI picking same formula. isoX=(x-y)*.5, isoY=-(x+y)*.25+z*1.5 with z=0 for ground, altitude for aircraft; orthoSize=24/zoom, local1280x720 RGBA only.
- Win by destroying enemy command core OR strategic suppression: THREE strategic nodes, activates at elapsed18min, control >=TWO for360secs. Contested nodes pause timer; losing majority rolls back at2x until0. Distinguish ore revenue from strategic victory progress. No forced winner at45min: duration is balance target, not arbitrary timeout.

## Building/rooms

- Drag variable rectangle foundation -> construct shell -> subdivide into rectangular rooms -> designate function -> install equipment -> connect actual networks.
- Shell dimensions4..24 cells each axis, increment1; adjacent expansion on the ground. Rooms may be smaller if their function requirements allow, but cannot be used to bypass shell4x4 minimum.
- Mixed-use rooms in every shell. Net usable area equals gross room area. GPU bays=floor(net usable area/4). Partition/merge cannot duplicate capacity/hp/refund. Expose room division/merge/conversion/expansion as actual operations/UI, and expose a space/utilization preview (net area -> capacity, cost per capacity, power output/load and compute capacity/demand before and after) built on the native preview RPC.
- Core starts construction drones. Construction takes time and a reachable route to the site edge; queued work, cancellation, repair and rubble clearing are playable. Jobs do not complete without access.
- Cost includes floor area/perimeter; HP area growth sublinear. Shell and room equipment have separate damage. Destroyed shells leave rubble that must be cleared.
- Room functions: data center, research lab, vehicle factory, airfield, anti-air control, network defense, energy defense, repair bay (aura), missile control, orbital control, wireless relay, data synthesis and the five branch facilities.
- Outdoor wind, hydro, coal, nuclear, extractor, mobile relay, airstrip and launch pad installations use appropriate foundation and geography. Airplanes take off/land at an airstrip.

## Economy/networks

- Starting credits2000; low core baseline income. Ordinary opening purchases must complete extractor, wind, DC+5060, initialresearchlab,2basicturrets and required wires with >=500 credits left. Verify the actual command sequence; do not just lower displayed prices or add money in tests.
- Credits build/buy; power runs equipment; compute AI/networkdefense/research; science data gates high tech. Maintain separate per-player and connected-component compute inventories.
- Finite ore nodes, richer central nodes. A completed extractor converts extraction directly into owner credits every second; destroyed extractors stop income. Coal/nuclear plants pay per-second credit upkeep while running.
- Units stop relevant behavior when their energy (mechanical) or compute (AI) runs out; there is no ammo or ground fuel. Aircraft endurance drains airborne and is restored on a powered airstrip.
- Power and compute isolated by owner and connected component. Lines break physically. Show ports, path, price, resulting connected supply/demand and distinct shortage reasons.
- Wired AI stays at endpoint; mobile relay provides wireless movement support. Outside coverage movement continues and attacks/skills consume onboard compute; no battery refill from disconnected distant network.

## Research branches

- Each lab owns ONE branch; player can build multiple branch labs. NO global faction hard lock and no arbitrary cross-branch purchase denial. Soft specialization from actual independent cost, time, data, facilities and continuing power/compute commitment.
- Initial lab CONSTRUCTION COMPLETE unlocks VSCode and PyCharm basic fixed turrets. Lab building itself cannot require a character. Power/compute required for further research.
- Speed: mobility/fire rate/rapid readiness/overclock, less durability.
- Security: intercept/anti-jam/defense, lower sustained damage.
- Algorithm: predictive aim/mark/penetration/precision/target choice.
- Science: highenergy particles/structure breaking/orbital, long setup and heavy upkeep.
- Lightweight: low purchase/upkeep/portable/dispersed, weaker individual HP.
- EVERY branch byT2 must possess anti-air and wallbreak counterplay. Common chassis + branchvariants; do not make single1-unit-per-tier branch unable to hit whole target classes.
- Science data mainly from contested nodes; inefficient local synthesis only as comeback path. FirstT5 normal expansion18–22min; secondT5 usually>=32min. Multiple labs may research concurrently (resource-limited), do not reduce to one globalResearch Option. Destroyed lab retains player knowledge, pauses new research/dependent high production; existing troops retain their ordinary weapons. Rebuild restores productive access.

## Exact AI roster / branch / mechanics

- T1 VSCode and PyCharm are common starter fixed turrets after initial lab; single-target precision vs cone/splash debugging.
- T2 Kimi SPEED: single-target burst, directional dash strike.
- T2 Claude SECURITY: precise attack, cone interception barrier.
- T3 GPT SECURITY: single-target shot, area repair + armor buff.
- T2 DeepSeek ALGORITHM: chain attack, directional penetration + mark.
- T2 Gemini SCIENCE: dual beam, telegraphed area bombardment.
- T3 MiniMax SCIENCE: cone shockwave, area repair/heal reduction.
- T2 GLM LIGHTWEIGHT: low-cost single shot, targeted support buff.
- All upgrade with their branch cap through T5. Skill range/direction/cooldown/compute validated; no arbitrary global clickdamage.
- Plugin categories core/attack/support unlock progressively. Branch and slot compatibility; no same-category stacking. Unit existing branch determines plugin compatibility.
- AI economic SOFT limitation, no fixed3/5 AI hard cap and no uniqueness. Approximately3x same-tier ordinary vehicle cost, sustained raw damage below same-budget mechanical squad, value comes from skills and mixed forces. Goal2–4AI mixed army, duplicates allowed.

## Weapons / defense

- Five smooth strategic tiers: T1 machinegun turrets/lightmortar/scout; T2 tanks/AA missiles/explosive breach equipment; T3 selfpropelled artillery/attack aircraft/rail accelerator; T4 particlecannon/longmissile/advancedbomber; T5 aerospace fighter/orbitalstrike/anti-orbital defense. Shared chassis with branch-specific weapons/modules, every branch operationally complete.
- Initial HP and sustainedDPS about1.4x per tier. Higher tiers add capabilities/range/deployment costs; lower troops remain useful.
- Unified actual impact resolution for units/vehicles/aircraft/structures/rooms/walls/links; direct, arc, guided, beam, delayed area. Ballistic projectiles damage on contact, not atfire+visualanimation. Physical occlusion by walls and shells, explosions attenuate to edge, moving/missing target handled.
- Network defense connectedDC: jamming guidance/network mitigation.
- Energy defense connectedpower: finite charge shield.
- CUDA data walls/moat: topology closed regions on the ground layer consume actual connectedcompute. Gaps in the wall ring matter, no free shield.
- Physical wall/armor: collision/cover/durability.
- Intercept/consume shield/leftover structuraldamage sequential, no infinite multiply-to-zero resist. Every valid impact damages applicable building/defense reservoir; after depletedshield positive structuraldamage with differing efficiency.
- Orbitalstrike telegraphed, expensive, requires destructible ground controller.

## Native/transport

- Rust V6 modularworld/construction/nav/network/economy/tech/combat/victory, struct commands and snapshots; no f32-digit encoding or sceneentities used as state DTO.
- Version/session/sequence/tick/rulehash/base snapshot IDs. Host authoritative60Hz; transport20Hz, local clientRurix render independent camera/interpolation. Replica never advances income/hp.
- LAN narrow game protocol, token derivesowner, original Forge MCP/scene/edit/viewport private localonly. Native revalidates permission/economy/idempotency.
- Disconnect keepsseat60s, battlecontinues, thenforfeit. Complete state save/load and deterministic command replay with versionvalidation. V5 saves untouched, no fake migration. Pre-flatten V6 saves are rejected by fingerprint, not migrated.
- Single-layer nav grid with local invalidation; viewcull not game-statecull. Rurix dynamicdepth/visibility/chunk draw support.

## Assets/UI

- New5 identities from verified larcgpt/ai-model-musume; ClaudeSonnet specifically. Keep sourceURL/author/version/SHA. DeepSeek/GPT retained but new coherent fullbodyiso actions. No invented Bili/YouTube identity; BiliZipZipPipe found but412 notvisualverified.
- 8 directions idle/walk/attack/cast/hit/death real localMiniMaxH3 I2V frames, immutable receipts+MP4. Main effects also trueframes. Model-bakedvehicles/buildingmodules allowed. No static/procedural sprite stand-in as finishedrequestedcharacteranimation.
- Modular foundations/walls/roof/equipment rather than stretching completeoldbuildingatlas. Each actualconstruction/damage triggers matchingvisual.
- Cardbased lobby/archive/resources/research/plugin; space/utilization preview card; minimalpersistentHUD. B/U/I,L/C,Q,Shift queue,Ctrl-numbergroups. Distinct targeting reticles for circle/cone/direction/ally buff.
- GPU particles optional extra; essential hits and skills complete frameassets regardless toggle.

## Mandatory acceptance (do not weaken to make green)

- Function tests for all above, public ordinarycommands for opening/research/economy/combat. Explicit fixtures can stress internal rules but not masquerade as earnedcampaign state.
- Matrix weapons vs4defenses, shells/walls/LOS/AA; hit-time damage, energy/compute-empty stop; ownership and separatednetworks; labprereq; branchplugin rejection; duplicatecommand/refund/repair/wall no resource duplication; split/merge capacityconservation; blockedroute recovery; z!=0 commands and saves rejected.
- Real economy matches each5branch pair >=20 mapseeds and swappedstarts, compare expansion/maintech/multitech/mech/mixedAI/turtle. Record duration/winrate/control/losses/sciencetime. Tune significant dominant strategies.
- TWO independent real clients+nativeprocesses fight45min, independentcamera, delay/duplicates/disconnectrecovery, consistentresult. Stubprotocol test alone is not combat acceptance.
- Pressure128shells/512functionalrooms/200activeunits/600projectiles on the single layer. Existing5060Laptop,1280x720>=30FPS, native tickP99<=16.7ms. No loweredthreshold/staticreplacement/truncation. Sampling H3 GPU must release before finalgameGPUtest.
- PortableV6 Windows isolatedcoldstart fullmedia/native/runtimefiles, standalonewithoutCodex, solo/create/join/save/replay verified. Reports separate functional/balance/performance/network/media evidence.
