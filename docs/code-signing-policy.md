# Code signing policy — proposed, pending review

Draft revision: 2026-09-14. This is the proposed policy for the [Code Sentinels V6 runtime component](../projects/code-sentinels/README.md). It is not an active signing policy, an application-success announcement or evidence of SignPath sponsorship. Foundation acceptance, service configuration and the first signing request remain pending.

## Responsible roles

All proposed roles use the confirmed public repository administrator's handle, [qwasg](https://github.com/qwasg):

- Maintainer: **qwasg — proposed role, pending review**.
- Author/committer: **qwasg — proposed role, pending review**.
- Reviewer: **qwasg — proposed role, pending review**.
- Signing approver: **qwasg — proposed role, pending review**.

One person is proposed to hold all four roles. Review and approval by that person must not be described as independent second-person review. Contributor and upstream copyright notices remain applicable; these assignments do not establish exclusive ownership of all dependencies. Changes from contributors without direct commit authority require recorded review before use in a signing build.

Non-sensitive project and policy questions use [GitHub issues](https://github.com/qwasg/RurixForge/issues), subject to GitHub and repository access rules. Private applicant contact details are not part of this policy. Do not submit secrets or personal documents in a public issue.

## Artifact scope

The proposed artifact is the project's formal Windows `engine-host.exe`, built from the exact reviewed public source revision by the approved GitHub-hosted workflow. That executable is a runtime component, not an asset-complete game distribution. The source review includes build scripts, dependency locks and the local Rurix patch; the relationship to upstream Rurix must be disclosed in the [notices and rights review](third-party-and-asset-rights.md).

A workstation binary, old test executable, fork/PR artifact or independently substituted file cannot stand in for the workflow artifact. Existing library-test executables are outside this production scope. Any development-test signing request needs a separately accepted scope.

Upstream `node.exe`, Microsoft runtime DLLs and other publishers' binaries must not be signed as this project's own code. Any later package needs an explicit component allowlist, applicable redistribution terms and preserved upstream notices. No complete portable package is approved by this proposal.

## Controls required before enabling signing

Repository and SignPath access must use MFA before production signing. The maintainer must verify repository protection, a GitHub signing environment with required approval, appropriate self-review restrictions where available, and a SignPath policy requiring manual approval of each request. Naming an environment in a workflow does not configure those controls. **Their current configuration has not been established by this draft.**

Unsigned build activation and signing activation are separate decisions. After source and publication review, the maintainer may enable the GitHub-hosted unsigned build for the approved source revision, reviewed build inputs and confirmed product/maintainer display identity. This build needs no Foundation grant or SignPath credential and can supply evidence for the application.

The later signing step remains disabled until Foundation acceptance, actual service identifiers and the required access/manual-approval controls are verified. Enabling an unsigned build does not authorize a signing request. Credentials belong in protected secret storage; no credential or private application identity belongs in source, logs, workflow inputs or release packages.

For each proposed signature, the approver must review the source commit, workflow run and attempt, dependency/source inventory, exact unsigned artifact identity, product metadata and permitted artifact configuration. The request must use that run's artifact ID. Approval is a recorded decision about one artifact, not a blanket authorization for future builds.

After signing, verify the actual trusted publisher and Authenticode result, required timestamp and PE metadata, signed file identity, unchanged provenance and the signed program's binding to the original unsigned content. The prepared byte-binding check deliberately permits only a narrow set of signing changes and may reject other legitimate layouts for manual review; it is not a replacement for Windows signature validation. Keep both unsigned and signed identities.

A signature does not replace startup, functionality, gameplay, balance, multiplayer or release review. Publication requires a separate review of the exact signed artifact and its notices. If identity, credentials or artifacts may be compromised, stop signing and release publication, preserve evidence and coordinate investigation and any required revocation with the signing service.

## Remaining decisions and attribution

The remaining items are the reviewed public source/release revision, completed build evidence, applicable license/asset decisions, verified access and approval controls, actual signing-service grant and configuration, and policy activation date. A release download URL will be recorded when a reviewed release exists; none is invented here.

**SignPath Foundation application and approval are pending; no sponsored signature or certificate is claimed.** If approval is granted, add the attribution required by the actual grant and [Foundation terms](https://signpath.org/terms.html) before using the approved service. Until then, this draft must not display sponsorship as an existing benefit.

Related documents: [Privacy policy](privacy-policy.md), [notices](../notices/README.md), [asset-rights ledger](../notices/asset-rights-ledger.json).
