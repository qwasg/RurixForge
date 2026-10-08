# Rurix GPU 粒子实验接线

此功能默认关闭。代码位于 crates/engine-host/src/gpu_particles.rs，使用现有 Rurix render_exec 的 Vulkan DeviceFrameSession；不是 CPU Sprite，也不宣称已接入完整 G35 系统。

## 开关

启动 engine-host（或会启动它的 RurixForge 进程）之前设置进程环境变量：

```powershell
$env:FORGE_GPU_PARTICLES = 'on'
```

变量未设置、off 或其他值时关闭。修改后须重启对应 engine-host；当前视口会话不支持在进程内热切换这一环境变量。实验绘制适用于普通 2D viewport，ModelRenderer 专用渲染路径未接此实验。

默认 off 不改变原有视频截帧技能特效。精美视频特效仍是主体；实验粒子可作为额外亮点装饰。

## 组件与游戏接口

为发射器实体添加启用的 ParticleEmitter 组件，props 为 {}。此组件已经注册到 forge-scene。实体没有 Sprite / MeshRenderer，故不会占普通实体 draw 槽。

- translation = [中心世界 x, 中心世界 y, 世界 z]。x 或 y <= -50 为池中隐藏事件。
- scale = [事件 age 秒, lifetime 秒, kind]。age >= lifetime、不合法数值或 lifetime <= 0 隐藏。
- kind：1 VS Code 青色穿透碎片、2 PyCharm 绿色喷散、3 DeepSeek 蓝色潮汐、4 GPT 紫色旋转。
- engine_game 的 CS_VFX0..63 使用 PublishEmitter 图及 cs_get(6000+slot*10+field)：field0/1 为中心，2 age，3 lifetime，4 kind，5 active。事件复用时 age 归零。
- 容量64发射器，每个64粒子，总4096实例。CPU每帧只上传64条事件记录，每条32字节；每粒位置、大小和颜色仅在GPU计算。

GPU compute 将解析轨迹写入粒子 SSBO；随后 vertex-pull 光栅 pass 以4096次实例化quad读取同一SSBO。两个pass有明确的buffer访问屏障，在普通场景绘制之后、共享纹理pack之前执行。默认生产帧不回读粒子buffer；测试额外回读用于证明GPU输出。

## 已完成的真实设备验证

首版在 Intel(R) Graphics 验证，补入真实加色管线后在 NVIDIA GeForce RTX 5060 Laptop GPU 重新验证。均为真实 Vulkan；设备创建或绘制失败会使手动GPU测试失败。

1. 正式加色版 GPU compute + draw 测试：age 0.25 / 0.75 两帧为1119 / 881个非黑像素；粒子SSBO内容随age改变，最终图像同时改变；事件过期后为0像素。
2. 完整viewport测试：实际 ParticleEmitter 组件经 render_scene_frame 接线，活动与过期事件产生不同画面。
3. off测试：关闭实验后同一活动/过期事件的viewport画面完全一致，证明关闭开关生效。
4. shader 编译与事件校验测试通过；普通draw容量扩到256，129/156/192/193/256实体不会因旧128上限而被提前裁掉。
5. forge-scene全部16项测试通过（包含更新后的组件注册表预期）。

证据文件位于同目录 artifacts/gpu-particles：

- gpu-particles-evidence.json：设备、compute/draw参数、SSBO与像素断言结果。
- viewport-emitter-evidence.json、viewport-emitter-off-evidence.json：完整viewport on/off证据。
- gpu-particles-0.png、gpu-particles-1.png、gpu-particles-2.png：直接GPU测试的活动两帧及过期帧。
- viewport-emitter-active.png、viewport-emitter-expired.png、viewport-experiment-off.png：完整viewport证据。
- 同名.rgba是原始GPU回读像素；PNG只是无损封装转换。

手动复跑（仓库根目录）：

```powershell
$env:FORGE_GPU_PARTICLE_EVIDENCE_DIR = 'D:/RurixForge/projects/code-sentinels/artifacts/gpu-particles'
cargo test -p engine-host --bin engine-host gpu_particles::tests::real_gpu_compute_and_draw -- --ignored --exact --nocapture
$env:FORGE_GPU_PARTICLES = 'on'
cargo test -p engine-host --bin engine-host gpu_particles::tests::real_viewport_emitter -- --ignored --exact --nocapture
$env:FORGE_GPU_PARTICLES = 'off'
cargo test -p engine-host --bin engine-host gpu_particles::tests::real_viewport_emitter_off -- --ignored --exact --nocapture
```

## 与 D:/Rurix 上游 G35 的区别

已直接阅读 D:/Rurix/rfcs/0049-gpu-particle-system.md、src/rurix-render/src/bin/g35_particle_core_device.rs、src/rurix-render/src/bin/g35_particle_lane.rs 和 src/rurix-rt/src/render_exec.rs。

上游 G35 真实粒子车道存在，包含sim→scan→compact→emit→indirect_args、splat与resolve、TSR/OIT等；其接线实现主要在bin-local G35OnLane中，生产车道追加32资源和10个compute pass，依赖自己的场景/深度/TSR协议，没有一个可直接调用的公共Emitter API。早期particle_core_device还注明每次run_compute派发重建设备，不能把该probe直接当实时游戏后端。

本次实验仅复用底层render_exec，对游戏事件运行解析GPU轨迹。初次检查发现上游 RasterPass 没有alpha blending接口；随后正式修复已经通过仓内vendor补丁新增真实Alpha/Additive混合，粒子现在使用带柔和透明边缘的加色GPU管线。具体补丁和像素对拍见 vendor/rurix/FORGE_BLEND_PATCH.md、artifacts/rgba-blend/sprite-blend-evidence.json。不声称物理碰撞、流体、OIT、百万粒子性能或完整G35确定性；这层粒子不会取代图生视频制作的主技能特效。
