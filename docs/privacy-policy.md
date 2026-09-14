# Privacy policy — Code Sentinels V6 review draft

Draft revision: 2026-09-14. Effective date: **pending review and publication**. Proposed accountable maintainer: [qwasg](https://github.com/qwasg), the confirmed public repository administrator. Non-sensitive questions use [GitHub issues](https://github.com/qwasg/RurixForge/issues), subject to GitHub sign-in and repository permissions. Do not include personal documents, credentials or raw diagnostic files in public reports.

## Scope and status

This draft describes the V6 gameplay paths inspected during source review. The proposed source release contains the native runtime and interface source; it does not include an asset-complete portable game. A final launcher, release artifact and this policy must be reviewed together before distribution.

These observations do not describe every RurixForge editor feature. Optional development, generation and agent integrations may contact services configured by their user. No claim that the entire editor is offline or makes no external requests is made here. No new traffic capture or runtime validation was performed to write this draft.

## Local gameplay

The inspected local V6 launcher uses a browser interface, a local bridge and the native engine. Actions, catalog data and snapshots pass through a loopback HTTP service; native RPC and image streaming also use local services. The inspected V6 client restricts its native image-stream address to loopback. Rendering frames are separate from multiplayer transport.

No configured central account, matchmaking, advertising, crash-upload or analytics endpoint was found in the inspected V6 gameplay paths. Those paths read game media rather than invoking the image/video services used during development. Windows, browsers, drivers, dependency tools and unrelated software have their own behavior and policies. This draft is not a guarantee about their network activity.

## Multiplayer selected by the user

Creating a room starts a game-protocol listener on network interfaces. Joining connects to the literal IPv4/IPv6 address and port entered by the user; a reachable public address is permitted, so the protocol is not limited to a private LAN.

The host and peer exchange room identifiers, nicknames, readiness, version/rule identity, player commands, heartbeat/reconnection messages, player-relevant state and results. Each endpoint's network stack observes connection and IP information. The authoritative host can retain its local replay journal; fog filtering is not a confidentiality guarantee against that host.

The inspected direct protocol uses HTTP/SSE without built-in TLS. Session credentials authenticate participation but do not encrypt the traffic. The user chooses the peer and network. Leaving closes that session's connections; the game does not automatically configure router or firewall forwarding.

## Local records and removal

The inspected paths use these local records:

- `.forge/save/v6/`: save names, creation times, mode, engine/rule identity and native save/order state.
- `.forge/replays/v6/`: host journals with room events, nicknames, orders, checkpoints and outcomes.
- `Logs/v6/`: native/bridge logs and launch records, which may contain timestamps, process IDs, service addresses, paths, orders/results and errors.

Live session credentials are held in memory by the inspected controller and are not intentionally added to normal save records. Logs and replay files can still be sensitive. Review and redact them before sharing; no universal guarantee about every possible error message is made.

No automatic upload or retention-expiry mechanism for these local records was found in the inspected V6 paths. They remain until removed by the user. For a reviewed portable release, close its processes, back up wanted saves and remove the extracted folder; the final release must document any additional paths it introduces. Removing local records cannot remove copies retained by a peer or voluntarily posted to another service.

## Websites, support and signing

Opening GitHub or another linked website sends requests to that service. GitHub's [privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement) applies to repository and issue use. An issue is a public feedback mechanism, not a private upload channel or a promised response service.

If the project later receives signing approval, the maintainer's CI will submit release artifacts and build metadata to the signing service. This is a release process, not a player-gameplay upload feature. See the service's current [privacy policy](https://signpath.io/privacy-policy) and this project's proposed [code signing policy](code-signing-policy.md). No signing enrollment or data submission is represented as completed.

Before activation, the maintainer must reconcile this draft with the exact selected release, verify startup/network behavior and retention paths, and set its effective date. Any future automatic telemetry, support-upload or hosted feature must identify its destination and data and update user controls and this policy before release.
