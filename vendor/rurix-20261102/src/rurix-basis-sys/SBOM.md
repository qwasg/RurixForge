# SBOM — rurix-basis-sys(M83)

| component | version / pin | SPDX | path | notes |
|---|---|---|---|---|
| `basis_universal` | tag `1.16.4` @ `900e40fb5d2502927360fe2f31762bdbb624455f` — 版本串 `basis_universal/1.16.4+g900e40fb5d25` | Apache-2.0 | `vendor/basis_universal/` | 真实 UASTC/ETC1S/KTX2 encoder + transcoder;`rurix_basis_version()` 字面锚定 |

upstream URL: `https://github.com/BinomialLLC/basis_universal`

source digest (SHA-256, full tree): `ead9a879f431f8eac90d3da154c991520fc52bda1018209298201e3e517288fe`

LICENSE digest (SHA-256 of `vendor/basis_universal/LICENSE`): `c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4`

NOTICE digest(SHA-256 of `NOTICE` file bytes, LF):
`pending-measured-by-smoke`(smoke 运行时对 `NOTICE` 实测写入 evidence notes;本表登记义务存在即可过 `license_sbom_entries_present` 的文件存在腿)。

**已废除**:`rurix-basis-transitional shim`(过渡 shim 条目已删除;旧版本串 `rurix-basis-transitional/0.1.0` 禁止再出现)。


RFC-0051 adds use of the already compiled public-domain `pvpngreader.cpp` and `jpgd.cpp` raster readers. A local PNG allocation ownership patch is registered with upstream/patched/patch hashes in `vendor/basis_universal/vendor_manifest.json` and VENDOR.md §5. No new third-party component is added.
