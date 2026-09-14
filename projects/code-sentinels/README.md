# Code Sentinels V6 — source review proposal

**Draft for publication review.** This page describes a proposed V6 source addition to [qwasg/RurixForge](https://github.com/qwasg/RurixForge). It does not announce a published V6 revision, downloadable game, accepted SignPath application or signed release. The repository's existing top-level README is retained.

Code Sentinels V6 is a strategy-game project implemented in native Rust with a TypeScript interface. The source-review scope includes its fixed-step simulation, the RurixForge `engine-host` runtime component, client source, dependency locks, and the repository-local Rurix runtime patch. The intended signing artifact is this project's source-built Windows `engine-host.exe`.

The candidate omits game artwork, original reference images, generated videos, saves, logs, private application data, existing executables and build caches. It is **not a complete playable game or portable distribution**. Source publication does not settle rights in excluded artwork or adaptations.

## Review and build status

Local source-component compilation has succeeded; the [local build summary](../../docs/local-build-summary.md) records the command, toolchain, unsigned artifact identity and limits. The exact public source commit, a GitHub-hosted build and any V6 release download remain pending. Dependency downloads may require network access. Hosted build inputs must come from the reviewed checkout and pinned dependencies, not a copied workstation executable.

The proposed product name is `Code Sentinels V6`; version fields follow Cargo package metadata. The proposed maintainer display identifier is `qwasg`, subject to review. It is a repository handle, not a claim of incorporation. After source and publication review, the unsigned GitHub build may be enabled to produce reviewable build evidence. Foundation approval is required for the later signing step, not for that unsigned build. Signing stays disabled until the service grant, configuration and manual-approval controls are verified.

## People and feedback

The confirmed public repository administrator is [qwasg](https://github.com/qwasg). The proposed maintainer, author/committer, reviewer and signing-approver roles are all assigned to that same handle, **pending role and policy review**. This is a proposed one-person arrangement, not an independent-review team or a claim of sole authorship of third-party code.

Use [GitHub issues](https://github.com/qwasg/RurixForge/issues) for non-sensitive feedback, subject to GitHub sign-in and repository permissions. Do not post credentials, personal documents or unredacted saves/logs. No private reporting channel or response-time commitment is claimed by this draft.

## Code signing policy and privacy

- [Code signing policy](../../docs/code-signing-policy.md): proposed artifact scope, accountable roles and approval requirements.
- [Privacy policy](../../docs/privacy-policy.md): inspected V6 data flows and the limits of this source-component proposal.
- [Third-party and asset-rights review](../../docs/third-party-and-asset-rights.md).
- [Dependency notices and inventories](../../notices/README.md).

SignPath Foundation approval and signing remain pending. No sponsorship, certificate or signed artifact is represented as granted.

## Licenses and remaining release decisions

RurixForge's code is declared under [Apache-2.0](../../LICENSE). The Rurix upstream dependency and local runtime patch retain their [MIT](../../vendor/rurix/LICENSE-MIT) or [Apache-2.0](../../vendor/rurix/LICENSE-APACHE) terms. Dependencies and assets keep their own terms; this page grants no rights in someone else's work.

Before a public V6 release, the maintainer must approve the exact source revision and notices, resolve the chosen asset set's rights, review completed build and runtime evidence, and configure and verify any signing controls. Release-download links will be added only when the corresponding reviewed artifact actually exists.
