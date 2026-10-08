# 官方模型订阅渠道

入口：设置 → 模型 →「连接你的模型订阅」。Codex、Antigravity（反重力）、Kimi Code、GLM Coding 四张紧凑卡片分别展示连接状态、额度窗口、重置时间和调用模型。宽屏四列，中等宽度两列，窄屏单列。

四个 Logo 均使用官网原始图标，保留原始颜色和形状。生图只用于独立的装饰背景，不代替 Logo。图标来源、哈希和配图提示词见 [渠道卡片视觉素材](channel-card-artwork.md)。

**Antigravity（反重力）**：点击「Google 网页授权」直接打开 `accounts.google.com` 的 Google Antigravity 登录与授权页面。浏览器回调到本机 `http://localhost:51121/oauth-callback`，后端校验本次随机 state，交换凭证、读取 Google 账户并发现订阅项目。卡片自动同步账户、真实模型目录和对应模型的剩余额度；完成后点击「使用模型」即可调用。未完成网页授权时保持“等待授权”，不会仅因打开页面而显示已登录。

本机连接组件使用开源 [CLIProxyAPI SDK v8.0.17](https://github.com/router-for-me/CLIProxyAPI/tree/v8.0.17)，负责订阅令牌续期、Google 模型协议、流式响应与工具调用转换。它不是 Google 官方 CLI；Google 的网页负责实际身份认证和用户授权。Forge 保留工具执行和权限控制。服务仅绑定 `127.0.0.1`，API/管理密钥只存在于内存，OAuth 凭证在 Windows 使用当前用户 DPAPI 加密，存于 `data/antigravity-home/credentials/*.credential`。服务重启会恢复凭证和账户模型目录。取消/超时会撤销本次凭证写入权限并清理等待中的授权，防止迟到回调重新建立连接。原有外部反代入口保留在卡片底部「反代配置」。

开发环境可运行 `pnpm antigravity:build` 预构建连接组件；首次授权缺少组件时也会自动构建，需要 Go 1.26 或更新版本。源码和固定版本依赖位于 `tools/antigravity-bridge`，许可证保存在同目录。Google 回调端口被占用时会提示结束其他正在进行的授权；不会接管其他进程。

首次启动时也可在登录页点击「连接官方模型订阅」直接进入卡片。已有官方渠道连接会满足本地模型的登录条件。

- **Codex**：点击「ChatGPT 官方授权」打开官方浏览器 OAuth；也可使用设备码授权。Forge 通过官方 `codex app-server` 调用模型、读取账户和用量。官方订阅使用 `data/codex-chatgpt-home`，与云网关的 CLI 配置分开；设置中显式指定的 `codexHome` 仍有效。连接成功后点击「使用模型」切换当前会话或下一次新会话的引擎和模型。
- **Kimi Code**：使用官方 Kimi CLI 的 OAuth、凭证刷新和模型 SDK。首次点击会自动安装 `kimi-cli==1.52.0`（需要 `uv`，使用 Python 3.13），完成后继续官方授权。运行时在 `data/kimi-runtime`，官方 CLI 凭证在 `data/kimi-home`。Forge 保留工具执行与权限控制，SDK 负责模型流式响应、思考内容和工具调用协议。授权取消会结束本次等待；过期链接需要重新授权。
- **GLM Coding**：打开智谱官方 Coding Plan 控制台登录，再在卡片内绑定套餐 API Key。此处采用智谱公开支持的 Coding Plan Key 接入方式；网页登录后不会读取浏览器 Cookie。Key 保存在本机凭证库（Windows DPAPI），配置文件只保存模型 ID。模型请求直连 `https://open.bigmodel.cn/api/coding/paas/v4/chat/completions`，独立于通用反代配置。

四个渠道的订阅额度来自官方接口；Antigravity OAuth 使用 Google `fetchAvailableModels` 返回的每模型 `quotaInfo`，选择模型时显示该模型的剩余比例和重置时间。外部反代模式沿用该服务的额度探针。缺失数据保持未知，不显示为剩余 100%。Codex 支持多个额度桶，Kimi 支持当前比例和旧版计数格式，GLM 使用官方 `/api/monitor/usage/quota/limit`，编程与 MCP 月度额度分开显示。接口未返回重置时间时显示「重置时间待同步」；刷新失败可前往官方控制台查看。

浏览器在点击时预留授权窗口，避免异步请求后的弹窗被拦截。桌面版使用外部浏览器桥接；已打开的旧桌面窗口也可使用原有外链处理器。只允许对应渠道的官方 HTTPS 域名，拒绝含凭证、伪造域名和自定义端口的授权地址。浏览器拦截窗口时卡片保留官方授权链接。

后端接口：`GET /api/forge/channels[/{antigravity|kimi|glm}]`，`POST /api/forge/channels/{id}/login`、`login/cancel`、`logout`。`config` 用于 GLM 套餐 Key，Antigravity 外部反代保留 `/api/forge/llm/antigravity/*`。Codex 保留 `/api/forge/codex/*` 接口。选择 `antigravity:<官方模型 ID>`、`kimi-code` 或 `glm-coding` 的会话固定路由到相应渠道；缺少凭证会立即报错，不会转到其他模型。

验证命令：

```powershell
pnpm --filter @forge/client typecheck
pnpm --filter @forge/client build
pnpm --filter @forge/client exec vitest run test/officialAuth.test.ts test/channelConnections.test.tsx test/settingsPages.test.tsx test/modelPicker.test.tsx
pnpm --filter @forge/host exec vitest run test/forgeProxy.test.ts
cargo test -p forge-agentd official_ -- --test-threads=1
cargo test -p forge-agentd antigravity -- --test-threads=1
go -C tools/antigravity-bridge test ./...
node tools/e2e/antigravity-adapter-smoke.mjs
cargo test -p forge-agentd codex:: -- --test-threads=1
& data/kimi-runtime/Scripts/python.exe crates/forge-agentd/tests/kimi_bridge_test.py -v
```

离线测试验证协议与路由；实际订阅登录需要用户在官方网页完成，账户额度和推理结果以登录后的官方响应为准。

参考：[Codex App Server](https://developers.openai.com/codex/app-server/)、[Kimi CLI Server API](https://www.kimi.com/code/docs/en/kimi-code-cli/reference/server-api.html)、[GLM Coding Plan 的 Codex 接入](https://docs.bigmodel.cn/cn/coding-plan/tool/codex)、[智谱官方额度查询脚本](https://github.com/zai-org/zai-coding-plugins/blob/main/plugins/glm-plan-usage/skills/usage-query-skill/scripts/query-usage.mjs)。
