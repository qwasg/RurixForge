The tiny legacy geometry fixtures `tri_min.gltf` and `quad_indexed.glb` are copied
from Rurix conformance/asset/gltf/accept at the workspace-pinned revision
1478859a8f2ea3a8e17abb06aa0211a6e0871cca. Keeping these fixtures here makes assetd
tests independent of a developer-specific H:/rurix checkout.

The richer PBR, skin, hierarchy and animation fixtures are constructed entirely
in `model_pipeline.rs`; no installed Blender is required for unit tests.
