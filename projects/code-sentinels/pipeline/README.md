# 角色真实媒体生产

角色视觉来自项目 `references/` 的网络出处核对；Codex ImageGen 只生成已核对形象的单帧定妆。

动作链路由 RurixForge 内 `agentEngine=codex` 会话实际执行：

1. 阿里云同账号、同模型的私有临时 OSS 上传参考 PNG。
2. 项目工作区 `ws_1788669812422_35134176` 的 `/api/forge/gen/video` 调用 MiniMax-H3，首帧图生视频 6 秒、768P。
3. `/api/forge/gen/video/frames` 通过真实 ffmpeg 解码、品红抠底、全帧共同裁切框，产出 8 fps、32 帧图集。
4. `verify_character.py` 检验原视频 SHA-256、32 个不同帧、相同尺寸与透明度，并从第一帧导出界面肖像。

图集：`public/assets/characters/{deepseek,gpt}.png`；同名 JSON 保存矩形 `[x,y,width,height]`、帧率、脚底中点 `pivot=[0.5,1]` 与供应商任务追溯信息。

GPT 源参考只有半身，生成提示明确要求维持半身投影，禁止补造腿脚。DeepSeek 保持蓝发女仆与鲸尾特征。两者动作使用生成视频的真实截帧，不以变形静态图或手绘伪帧代替。

## 运行

在本项目 RurixForge Codex 会话中执行：

```powershell
./pipeline/generate-character.ps1 -Character deepseek
./pipeline/generate-character.ps1 -Character gpt
```

该脚本只向本机可信媒体服务传递预上传的私有引用，不需要外部 Python 或读取密钥。`*.video.json` 已存在时复用同一个真实视频；`*.attempt.json` 已存在但没有成功回执时拒绝盲目重复付费请求。确认供应商终态失败并修复原因后，保留旧 attempt/failure 为带原任务 ID 的文件，再显式重试。

验证与界面导出由主任务执行（包含写入客户端静态资源目录）：

```powershell
python pipeline/verify_character.py deepseek
python pipeline/verify_character.py gpt
```

`pipeline/python-libs/` 是已忽略的本项目依赖目录。RurixForge 的 ffmpeg 位于仓库 `data/tools/ffmpeg.exe`，版本 7.1。

## 可追溯记录

`codex-session.json` 和 `gpt.codex-session.json` 记录真实会话身份。完整引擎工具轨迹位于仓库 `data/agent-events/sess_1788670123057_5380645c.jsonl` 与 `sess_1788670201477_0bd1ccb8.jsonl`。`*.codex-result-after-activation.json` 记录本项目代理的最终响应。

首次真实供应商任务 `7eb031a3-02bc-4090-982b-95cc5186e2b6` 因 MiniMax 服务未开通失败，原始回执以 `deepseek.*.failed-7eb031a3.json` 保留。用户随后确认已经开通，再启动新任务；没有把失败结果当成媒体资产。

本次修复并验证了媒体 REST 工作区作用域（防止新项目资产错误落入 demo），以及官方私有 OSS 引用必需的 `X-DashScope-OssResourceResolve` 请求头。对应工作区回归测试 1/1、MiniMax 适配测试 5/5、截帧核心测试 7/7 通过。

## GPT 第二版质量验收

首版少数帧的展开翼尖触及画面边缘，未作为最终质量状态保留。新首帧 `Content/Concepts/gpt-dragon-v2.png` 保持原网络形象并增加品红安全边距，再由原项目 GPT Codex 会话生成真实第二版视频（任务 `f35fadac-a33e-4390-ac27-7279bd5694d8`）。

`verify_video_margins.py` 使用与 gend 一致的品红规则检查完整原视频的全部 158 帧：左、上、右、下最小前景边距分别为 **102、174、98、174 像素**，所有帧均超过 10 像素门槛。最终 32 帧图集每帧 566×420，像素哈希全部不同，已逐项核验半身投影与完整翼尖。

最终 `gpt` 视频、图集、界面肖像及证据采用第二版。第一版原视频另存 `SourceMedia/gpt-v1.mp4`，源数据、图集、肖像和完整工具轨迹保留在 `pipeline/versions/gpt-v1/`；第二版原视频另有 `SourceMedia/gpt-v2.mp4`。`gpt-v2.margins.json` 保存每一帧的 bbox 与四边距离。

官方接口资料：[MiniMax 图生视频](https://help.aliyun.com/zh/model-studio/minimax-video-generation-api-reference)、[模型绑定的私有临时上传](https://help.aliyun.com/zh/model-studio/get-temporary-file-url)。
