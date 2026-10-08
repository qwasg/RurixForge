# 高算力技能真实视频特效

三项动画由本项目 `agentEngine=codex` 会话实际调用 MiniMax-H3 图生视频生成。输入为 `Content/Concepts/v2/` 下已确认的三张黑底特效定帧，原始视频永久保存在 `SourceMedia/effects/`，没有静态特效或程序假帧替代。

每段请求 6 秒，供应商实际输出 6.58 秒、24fps、158 帧。gend/ffmpeg 以 12fps 解码前六秒的 72 帧，再等时选择其中 48 个真实帧，没有生成中间插值帧。`selectedSourceFrames` 提供逐帧索引追溯。

`public/assets/effects/` 下三个 PNG 图集均为 1808×1808，包含 48 个 256×256 帧和 2px 透明间隔，单图 RGBA 显存为 13,075,456 字节。对应 JSON 保存 `boxes`/`frames=[x,y,w,h]`、中央 pivot `[0.5,0.5]`、24fps/2s 的不循环 `oneshot` 和可选循环 `idle` 参数。所有像素为 straight-alpha，可使用 SRC_ALPHA/ONE 加色融合；普通 alpha 合成也没有黑色矩形背景。

## 黑底与边缘处理

本次为 `gend::video_frames` 增加 `chromaKey=black`。RGB 峰值作为透明度，颜色反预乘；峰值≤3 的编码黑噪声清零。这样保留蓝、紫、绿能量的色相和软辉光，不会像品红色键那样误删紫色特效。REST 与 gen-image MCP 枚举已同步。8 项截帧核心测试通过，包括将能量重新合成到黑底后通道误差不超过 1 的验证。

潮汐与星核的视频主体轮廓完整，但生成模型在爆发时附带了到达边界的细镜头光线。按主任务确认的贴图准备流程，仅每帧最外 25px（9.765625%）施加以下常规透明羽化：

`alphaOut = alphaIn × smoothstep(0, 25, min(x, y, 255−x, 255−y))`

RGB 全图完全不变，中央 206×206 区域 RGBA 逐字节完全不变，外缘四条像素线 alpha 均为 0，均已逐帧自动验证。没有新增或替换动作帧。矩阵原视频已有 72px 以上安全边距，无需羽化。`unprocessed/` 永久保存三个羽化前的 48 帧图集和帧索引，原始视频也保持未修改。原视频的镜头光线边界记录保留在 `*.source-validation.json`，未被改写成通过。

`*.terrain-composite.png` 展示在深色、浅色地面样板上的真实 alpha 合成，已经视觉核对没有黑框或硬裁边。`*.contact.png` 为真实时间序列联络表。潮汐不同帧数为45，最后部分帧是供应商真实消散成黑的空帧；其余两项均48帧不同。

## 重现与证据

- `dispatch-effect.py`：通过项目 Codex 模式发起特效生产。
- `generate-effect.ps1`：私有参考引用 → 项目媒体 API → 真实视频 → gend 黑底截帧；遇到既有 attempt 拒绝盲目重复付费。
- `package-effect.py`：实际帧等时采样、RGBA处理验证、256px缩放与图集打包。
- `inspect-effect.py`：完整解码并记录所有158帧的边距与能量消散。
- `archive-effects.py`：持久化供应商任务ID、视频及图集SHA-256、项目会话/run、关键工具执行全过程。

`evidence.json` 为本媒体子流程的最终验收；游戏内部的导入GUID、逻辑和胜负测试由游戏实施任务维护。
