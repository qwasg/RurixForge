# Blender model runtime

The upstream Rurix dependency stays at `1478859a8f2ea3a8e17abb06aa0211a6e0871cca`.
Run `./scripts/bootstrap-rurix-physics.ps1` before building on Windows. It locates
the installed Visual Studio CMake and restores the one omitted Jolt build file
from the exact vendored commit, with a pinned SHA-256 check. It only changes the
Cargo dependency cache; it does not patch physics or change repository pins.

## Runtime components and tools

- `ModelRenderer { model, nodeId?, materialOverrides?, revision? }` loads the
  assetd model bundle. `nodeId` selects one stable source node's primitives;
  omitted/empty renders the full model. `revision=0` follows current publication.
- `Parent { entity }` stores local scene hierarchy. Blender templates use a
  container plus stable source-node entities. The source transform is applied
  exactly once; editing a child affects rendering, picking, and map collision.
- `Animator { clip, idleClip, walkClip, time, speed, playing, loop, manualControl }`
  evaluates glTF TRS curves and CPU skinning. Each instance has its own pose and
  vertex buffer. Normal and tangent directions follow the deformed geometry.
- `CharacterController { speed, radius, height, controlled }` moves a capsule in
  XZ, applies Y gravity, and sweeps/slides against map colliders. Existing
  left/right/up/down input actions drive it. Idle/walk follow actual movement.
  Explicit animation controls set `manualControl=true`; set it false to return
  to movement-driven clips. Jumping, stair stepping, IK and root motion are not
  part of this basic controller.
- `Collider { shape: "mesh", model }` uses explicit collision-marked nodes when
  present, otherwise the static model meshes. Child-instance transform overrides
  also affect its world-space collision mesh. Box/capsule shapes are available.

MCP tools map to the corresponding host RPCs:

- `prefab_instantiate` → `prefab.instantiate { prefabRef, translation?, rotation?, scale? }`
- `prefab_revert` → `prefab.revert { id }`
- `asset_reload` → `asset.reload { guids?, revision? }`
- `animation_control` → `animation.control { id, action: play|pause|stop|seek, clip?, time?, loop? }`
- `template_preview` → `template.preview { prefabRef, width?, height?, clip?, time?, yaw? }`

Previews return real GPU RGBA8 pixels as `pixelsB64`, plus width, height, device,
draw/triangle counts. Yaw is radians. Scene viewport transport is unchanged;
the standard model renderer currently returns GPU readback frames for the
existing presentation path instead of importing a D3D12 shared texture.

## Updates and materials

Prefab instances retain their root placement, stable source identities, and a
baseline. Reimport performs a three-way property merge: untouched properties
update, local changes stay. Removed source nodes with local changes remain and
produce conflicts; their renderer pins the previous archived model revision,
including embedded textures. Revert discards overrides and removed-node copies.
Instantiation, deletion of entity subtrees, update and revert are undoable in
edit mode. A reload during play rebuilds the collision world atomically and
reports `physicsRestarted`; running rigid-body velocities are reset.

`materialOverrides` maps material-slot strings to per-instance PBR parameter
objects (`baseColor`, `metallic`, `roughness`, `emissive`, `normalScale`,
`occlusionStrength`, `alphaMode`, `alphaCutoff`, `doubleSided`).
Base color, normal, metallic/roughness, occlusion and emissive textures are
sampled using model UVs. Repeat, mirrored-repeat, clamp, nearest and linear
sampling are supported; mip chains are not generated. OPAQUE/MASK draw directly;
BLEND primitives sort back to front and use a GPU alpha-over composition pass.
As with primitive-sorted transparency, intersecting transparent geometry is not
an order-independent transparency solution. Legacy-only 2D scenes keep their
existing sprite rendering path and chroma-key behavior; the new PBR shader
retains purple textures.

Per frame, the model path permits 2048 draw primitives, 256 MiB of expanded
vertex data, and 256 MiB of unique packed texture data. Limits are checked before
allocating. Identical packed material textures share CPU and GPU storage.
Exceeding a budget returns `MODEL_BUDGET`; it never replaces a model with a cube.

## Verification

```powershell
./scripts/bootstrap-rurix-physics.ps1
cargo test -p engine-host --bin engine-host -- --nocapture
cargo test -p forge-scene --lib
cargo test -p engine-scene-mcp --bin engine-scene-mcp
cargo build -p engine-host -p engine-scene-mcp
```

The model GPU acceptance test requires an actual Vulkan device. It checks UV
color regions, independent skinned instances, same-size texture updates, retained
purple material pixels, nearest sampling, alpha-over pixels, and 129 draws with
zero mesh fallbacks. Other self-contained tests cover node transforms/picking/
collision agreement, capsule ground/wall collision, prefab updates/overrides/
deletion conflicts, and scene serialization. No external `H:/rurix` fixture is
needed for these tests.
