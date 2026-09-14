# Standalone game device preference

V5 runtime acceptance on a hybrid Intel / NVIDIA laptop found that the render
executor always selected the first enumerated Vulkan device, which was Intel
Graphics. `src/rurix-rt/src/render_exec.rs::pick_physical_device` now recognizes
the process-local opt-in `RURIX_PREFER_DISCRETE_GPU=1` and chooses the first
device whose Vulkan device type is `DISCRETE_GPU` (2). Without that opt-in the
existing first-device policy is unchanged; machines without a discrete device
continue to use their first Vulkan device.

The portable game bridge supplies this environment variable only to its owned
engine process. No Windows graphics preference, system proxy, D3D12 adapter
selection, driver configuration or editor default is changed. Native device
names and actual frame/FPS evidence remain part of runtime acceptance.

This patch is additional to the existing Forge alpha/additive raster patch.
