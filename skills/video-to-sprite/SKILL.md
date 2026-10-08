---
name: video-to-sprite
description: 将已有视频截成角色动画帧、透明精灵图集或 .rxsprite 动画。当任务涉及视频截帧、视频转精灵、从动画视频提取走路或待机帧时使用；自动检查解码依赖并调用 RurixForge 的截帧实现。
---

# 视频转精灵动画

将用户指定的视频转成真实帧图集。截帧、抠底、裁切和拼图通过现有 `gen_video_frames` 实现；解码依赖由技能内部检查。

## 执行流程

1. 确认视频来自当前工作区，保留原始视频。确定用户需要帧图集还是可播放的角色动画；已有视频无需再次生成。
2. 调用 `GET /api/forge/tools/ffmpeg` 检查解码依赖。未找到时检查 `<workspace>/data/tools/ffmpeg.exe`、`FORGE_FFMPEG` 和 PATH；依赖不可用则明确报告该步骤无法执行。用户只需提供视频和截帧目标，无需管理“外部工具”。
3. 调用 `mcp__gen-image__gen_video_frames`，工具参数为：

   ```json
   { "videoFileRef": "<视频引用>", "fps": 8, "maxFrames": 32, "chromaKey": "auto", "crop": "union" }
   ```

   也可调用 `POST /api/forge/gen/video/frames`；REST 请求同时传入当前 `workspaceId`。按用户要求调整 fps（1–30）与 maxFrames（2–256）。纯色底用 `auto`，品红底用 `magenta`，保留背景用 `none`。默认 `union` 让所有帧共用包围盒，避免角色脚底抖动。
4. 检查返回的 `frameCount` 与 `boxes` 数量、MCP `frames` 映射条目数一致且至少为 2。REST 响应的图集引用为 `atlas.fileRef`，每个 box `[x, y, width, height]` 按顺序映射为 `frames.frame_<i> = { bbox: box }`；超过 10 帧时编号补齐两位（`frame_00` 起）。检查图集预览，确认动作顺序、透明底和主体完整。抠空或帧数不足应报告真实错误；保留视频，不能用重复静态图伪造动画。
5. 用户只要求截帧时，返回 MCP 的 `atlasFileRef`（REST 使用 `atlas.fileRef`）、帧数、帧率与预览。需要入库时调用 `mcp__gen-image__gen_accept`，使用图集引用、`destFolder: "Textures"`、用户指定名称与 `origin: "gen-video"`。
6. 用户需要角色动画时，用返回的 `frames` 显式调用 `mcp__asset-pipeline__sprite_create`，贴图引用使用入库 GUID。不要再次自动切帧；角色分离的部件可能被误判为多帧。
7. 使用 `sprite_set` 定义动作 clip，帧列表按返回顺序、默认 8 fps。走路或待机默认循环，一次性动作采用 `loop: false`。地面角色使用脚底中心 pivot `[0.5, 1]`，飞行角色和特效使用 `[0.5, 0.5]`。
8. 修改场景前读取现状并建立 checkpoint。将动画挂到用户指定实体，试玩检查帧播放、脚底对齐和循环衔接，保存后交付。只有图集请求时无需修改场景。

## 输出约束

报告原视频引用、图集引用、实际帧数和 fps；有入库或动画时附 GUID 和 clip 名。结果必须来自真实工具返回和预览。

## 失败回退策略

保留原视频与已有资产。截帧失败时说明失败步骤与实际错误，修正参数或依赖后重试。场景修改失败时用本次 checkpoint 回滚，不修改不相关实体。
