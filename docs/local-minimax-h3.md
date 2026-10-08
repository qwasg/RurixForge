# 本地 MiniMax H3 视频工作流

生成后端 `comfyui-minimax-h3` 调用本机 ComfyUI，支持文生视频和首帧图生视频，输出包含原生音轨的 MP4。设置与云端后端分开保存，不需要 API Key。

## 使用

1. 启动本地 ComfyUI。本机安装目录为 `E:\MiniMax-H3`，可双击其中的 `start-h3.cmd`；默认服务地址为 `http://127.0.0.1:8188`。
2. 在 RurixForge 的“设置 → 模型”生成后端中启用“MiniMax H3 · 本地”，保存本地服务地址。
3. 在“素材创作”新建视频节点，选择“MiniMax H3 · 本地”。已启用本地后端时，自动选择会优先使用它；已有显式云端选择保留。
4. 输入提示词后生成。选择项目中的参考图或连接上游图片节点时，自动走图生视频。

本机 RTX 5060 Laptop 8GB 默认使用 `352p / 2 秒 / 16:9` 预览：608×352、56 帧、24fps，实际约 2.33 秒。帧数按模型要求对齐，产物元数据同时保留请求时长与实际设置时长。可选 480p、768p，以及 9:16、1:1；更大的设置需要更多计算与显存，完整 2K 托管流程不在这个本地后端中。

## 接口与文件

继续使用现有 `POST /api/forge/gen/video`，由 Host 转发到 agentd。例如：

```json
{
  "backend": "comfyui-minimax-h3",
  "prompt": "窗边的小盆栽在微风中轻轻晃动，阳光照亮木桌，伴有鸟鸣。",
  "resolution": "352p",
  "durationSec": 2,
  "aspect": "16:9"
}
```

工作区请求保留 `workspaceId`。参考图可用项目内 `imageRef`，由现有路径校验转换为图片 data URI，再上传至本机 ComfyUI；不上传到云端。后端仅接受回环地址的 HTTP(S) 服务，以及 PNG/JPEG 参考图。

视频任务固定使用发起时的工作区。生成期间切换画板，结果仍写回原画板；返回后可查看完成的版本。刷新后的历史视频通过 `GET /api/forge/gen/video/file?workspaceId=...&fileRef=...` 读取项目文件，该接口支持字节范围请求和播放器拖动进度，不依赖 localStorage 保存整段 base64。

每次生成先检查原生节点和五个模型文件名，再提交一次 `/prompt`、轮询对应的 `/history/{prompt_id}`，最后下载该任务输出节点的 MP4。不会自动重新提交或切换到云端。提交后的超时和失败消息包含任务编号，可在 ComfyUI 查询原任务。

- 非密配置：`data/gen-backends.json`
- 提交与结果回执：`data/comfyui-h3-tasks/`
- 版本化工作流：`crates/gend/src/media/comfyui_h3_v1.json`
- RurixForge 产物：目标项目的 `.forge/tmp/gen/`，通过既有素材版本和预览流程使用

`configured` 仅表示配置有效；服务停止或缺少权重时，生成会返回明确错误。回传的尺寸、帧率和音轨声明来自提交的原生工作流，实际文件验证记录另行保存，避免将设置值误当成已探测的媒体信息。

## 本机集成验证

验证记录存放在 `evidence/local-h3-integration-20260909/`。只有实际经 RurixForge 返回并成功解码的视频，才记录为真实生成通过。

2026-09-09 已从 RurixForge 页面完成两类真实生成，结果均写入“编译防线 · Code Sentinels”项目：

- 文生视频：608×352、56 帧、24fps，ComfyUI 执行 56.75 秒。
- 图生视频：选用项目 `Content/Concepts/gpt-dragon.png`，352×352、56 帧、24fps，执行 45.64 秒。上传到本机的图片 SHA256 与项目原图一致。
- 两段视频均有 32kHz 双声道音轨，全部视频帧和音频采样解码成功，RurixForge 文件与 ComfyUI 原产物逐字节一致。
- 完整文件与 Range 请求均经 Host 实测，分别返回 200/206 且字节校验一致；刷新页面后历史视频已实际播放。
- 定向自动化测试：gend 媒体 27 通过（另 1 项远程实时测试忽略）、agentd 13、Host 19、客户端 95；相关类型检查和生产构建通过。

本次同时修正旧视频调用缺少工作区 ID 的问题，及切换画板时视频结果可能写入错误节点的问题。已有云端后端配置保留；本地后端失败不会自动调用云端。
