# Local source-component build summary

Review date: 2026-09-14. This summary records one completed local compilation of the 657-file source component. It is public-safe build evidence; it contains no workstation path or private application contact.

The ordinary build command completed successfully with exit code 0:

```sh
cargo build --locked --release -p engine-host
```

The build used Rust 1.93.1 and Cargo 1.93.1, the existing CMake installation and Windows SDK, and existing Cargo dependency caches. Its target/output directory was separate from the source candidate. The recorded source manifests were identical before and after compilation. This was a local build using available dependencies; it does not establish a fresh-cache or hermetic build.

## Unsigned artifact identity

- Artifact: `engine-host.exe`.
- Size: **12,334,080 bytes**.
- SHA-256: `509cd0f038597f8fcfb44c1c2415a071eb3fb67bff2e62f5f071b883073630d6`.
- ProductName: `Code Sentinels V6`.
- ProductVersion: `0.1.0`.
- FileVersion: `0.1.0.0`.
- CompanyName: `qwasg`, the public maintainer display identifier.
- Authenticode status: **NotSigned**.

These PE fields were inspected without launching the application. The compiled application was **not executed**. This result is **not a GitHub-hosted CI run, a signature, or acceptance of a complete playable package**. The source component still omits the complete game asset set; compilation does not settle asset rights, runtime behavior, focused test execution, gameplay, balance, multiplayer or release approval.

A separately authorized unsigned GitHub build can establish hosted source-build evidence after source/publication review. Any later signing request requires its own Foundation and policy approvals. This local executable is not substituted for the hosted workflow artifact.

See the [source review overview](../projects/code-sentinels/README.md) and [code signing policy](code-signing-policy.md).
