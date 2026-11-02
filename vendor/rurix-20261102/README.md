# Rurix library snapshot for RurixForge 0.2.0

This snapshot makes the 2026-11-02 Forge desktop build independent of local
`D:/Rurix` path overrides. It contains the library dependency closure from
Rurix base commit `86dff3706006bfdba5995b840d6834533b69905c`, including local library changes not yet
available in upstream GitHub. `SNAPSHOT.json` records every copied input hash
and the manifest-only packaging adaptations. Library implementation files are
copied byte for byte. Standalone experiment binaries and their large kernel
catalogue are omitted; the runtime build kernels and literal library includes
are retained. The patched Naga 25.0.1 source is retained in the sibling folder.

This is a source snapshot for the desktop release. It does not change the
separate renderer project's paused work or claim its experimental render
quality and performance gates have passed. Upstream licenses are retained.
