# Proposed native source build and signing workflow

This is an unpublished source-review configuration. The workflow has the `.yml.disabled` extension, source approval is false, and signing is off. No hosted build, signed artifact or complete game release is established by these files.

The proposal builds only the project-owned `engine-host.exe` with normal `cargo build --locked --release`, Rust 1.93.1 and a GitHub-hosted Windows 2022 runner. Existing CMake, MSVC and Windows SDK tools are checked before compilation. A fresh short `RUNNER_TEMP/spv6/t` output directory is used. The scripts do not launch the compiled runtime or test executables. Artwork and a complete portable game are outside this artifact scope.

## Unsigned build activation

After the maintainer approves the exact published source and notices:

1. Set `sourceReleaseApproved=true` in `.signpath/v6-native-inputs.json`, and repository variable `V6_SOURCE_RELEASE_READY=true`.
2. Confirm repository variable `V6_RELEASE_COMPANY_NAME`. PE metadata must match the reviewed project identity and actual Cargo version.
3. Rename `.github/workflows/signpath-v6.yml.disabled` to `signpath-v6.yml` and place the enabled workflow in the repository's default branch so GitHub registers `workflow_dispatch`.
4. Dispatch an unsigned build with `request_signing=false`. Keep Foundation approval false and signing credentials/identifiers unset.

Publishing a source branch alone does not enable the manual trigger. These activation steps have not been performed for this review package. A hosted build has different compiler-image and dependency-environment inputs from the local compilation; bit-identical output is not claimed.

## Later signing activation

Foundation acceptance is a separate later prerequisite for signing. Verify MFA and the actual repository/environment approval controls, configure a SignPath policy that requires manual approval of each request, and store the granted identifiers and credential only in the protected environment. An environment name in YAML does not itself establish reviewer protection.

Only after those controls and the actual grant are verified may `SIGNPATH_FOUNDATION_APPROVED=true` and a version-tagged dispatch with `request_signing=true` be used. The connector must verify the build's origin. Sign only this project's own `bin/engine-host.exe`; upstream binaries and existing local test programs are outside this configuration.

The scripts verify Windows signature status, expected signer, PE metadata, provenance and narrow preservation of the original program bytes after signing. The byte check is deliberately conservative and is not a complete Authenticode image-hash implementation. Any unexpected signing layout requires review.

Compilation or signing does not complete runtime, gameplay, balance, multiplayer or release acceptance. The workflow does not publish GitHub Releases. See the [proposed code signing policy](../../docs/code-signing-policy.md), [privacy policy](../../docs/privacy-policy.md), and [source-review scope](../../projects/code-sentinels/README.md).

Official requirements: [Foundation terms](https://signpath.org/terms.html) and [trusted GitHub builds](https://docs.signpath.io/trusted-build-systems/github).
