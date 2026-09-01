# 资产商店与 Skill 管理 Spec

## Why

Sidebar 目前只有会话域入口（New Agent / WORKSPACES / CHAT FOLDERS），资产与技能两类可复用产物没有任何分发与管理面：资产只能从本机磁盘 `asset_import` 逐个导入，技能只能手工在 `skills/` 建目录写 Markdown。这让「沉淀一次、跨项目复用」在产品上不成立，也没有社区协作的入口。

更严重的是一处**文档与实现的断层**：`06_SKILLS_LIBRARY.md §2` 声称「agentd 启动扫描 skills/ 生成可用技能索引注入系统提示，agent 调 `read_skill(name)` 取全文」，但全仓 grep `read_skill` 只命中三处文档（`06`、`skills/skill-creator/SKILL.md`、`milestones/f3/F3_CONTRACT.md`），代码中零实现。实际链路是 `packages/client/src/components/chat/Composer.tsx:199-204` 把选中技能名拼成 `Use skills: a, b.\n\n` 文本前缀，服务端 `crates/forge-agentd/` 全域不解析这个前缀——**SKILL.md 全文从未进入过 LLM 上下文**。用户在 UI 里勾选技能，对模型没有任何实质作用，属于静默失效（违 I-5）。

本 spec 一次性交付两个大类，并在此过程中兑现 `06 §2`。

## What Changes

### 商店（可插拔 Registry 协议）

- 新建库 crate `crates/forge-store`：包清单 DTO 与校验（`manifest.rs`）、`RegistrySource` trait 双实现（`FileSource` 走 `file://`、`HttpSource` 走 `https://`，复用既有 `ureq 2`）、多源聚合与源配置（`registry.rs`）、个人资产库内容寻址 blob store（`library.rs`）、安装卸载更新（`install.rs`）、打包发布（`publish.rs`）、skill 包辅助（`skillpkg.rs`）。
- 传输走 **sha256 内容寻址逐文件**，不引 zip/tar 依赖：与 `crates/forge-agentd/src/pack.rs` 既有「包 = 目录而非归档」形态对称，且天然去重/可校验/可断点。代价（小文件多时请求数高）如实登记，协议留 `bundleUrl` 可选字段备后续。
- 安装复用 `assetd::import::import_assets` 完整构建链，落地后覆写 `.meta.provenance.origin = "store-install"`（照 `crates/gend/src/accept.rs::accept_asset` 的三步法）。
- `sha256_hex` 抽进 `crates/forge-util`（新 `hashutil.rs`），消除第四份重复实现（既有三份在 `assetd/meta.rs`、`forge-index/lib.rs`、`forge-logic/callruntime.rs`）。
- 新建 `crates/mcp/store-mcp`（照 `crates/mcp/context-mcp` 模板）+ agentd `src/store.rs` REST 面。长耗时安装走 REST（host 代理 `isLongLivedPath` 豁免）+ MCP 侧异步 `taskId` 轮询，原因是 `crates/forge-agentd/src/mcp.rs` 的 `CALL_TIMEOUT` 为 10 秒。
- 仓内 `registry/` 为默认官方源（`file://` 驱动），离线可完整验收。

### Skill 管理

- agentd 抽出 `src/skills.rs`：frontmatter 解析器由「只认 `name:`/`description:` 两行」重写为结构体解析（新增可选 `version` / `license` / `tags` / `allowed-tools`，向后兼容既有 13 篇）；配置改原子写；`list`/`read` 名字口径统一。
- 新增 CRUD 与 validate 端点；删除为 destructive，须 approved Proposal（`kind=skill.delete`）。
- **注入链路实装**：`read_skill` 原生工具（`NATIVE_TOOLS` + `runtime_tool_specs` + `dispatch_native` 三处）；系统提示注入「可用技能」索引（仅启用项的 name + description）；`ask:execute` 新增结构化 `skills[]`，选中技能全文经 F10 已验证的 preamble 通道作独立 system 消息注入，并发 `agent.skills.injected` 事件留痕。
- skill 可经商店分发（`kind: skill` 包落 `skills/<name>/`），分发物仅 SKILL.md 文本。

### 前端

- `Sidebar.tsx` 在 `sidebar-new-agent` 下方新增两个导航按钮（同款样式）。
- `workbenchStore.ts` 新增 `store` / `skills` 两个 `BuiltinTabKind`，`Workbench.tsx` 分派，`commands.ts` 加命令。
- 新建 `StoreTab`（发现 / 已安装 / 我的资产库三子页 + 源管理 + 搜索过滤 + 卡片网格 + 详情页 + 安装进度）与 `SkillsTab`（列表 + 启停 + 新建/编辑/删除 + 全文编辑器 + 校验提示 + 目录配置）。
- `Composer.tsx` 文本前缀下线，改发结构化 `skills` 字段；顺带修既有缺陷：菜单不按 `enabled` 过滤（`Composer.tsx:546`）。
- 设置页 skills tab 收敛为「目录配置 + 跳转」，不再重复列表（避免两处事实源）。

### 明确不做（诚实登记，留用户拍板）

- 支付/结算流程：manifest 保留 `pricing` 字段并展示，`amount > 0` 的包安装时显式 `STORE_PAYMENT_REQUIRED` 拒绝，不伪造购买态（defer `RD-F11-001`）。
- 账号体系/发布者认证/内容审核：`publisher` 为自证信息不校验，私有源经 keystore token 鉴权（defer `RD-F11-002`）。
- 云端 registry 服务端实现：协议已定义且 `HttpSource` 已实装，服务端属独立工程（defer `RD-F11-003`）。
- hooks/plugins 市场 UI：维持 `RD-F7-002`，D-025 已划界。
- 商店内交互式 3D 预览器：复用既有缩略图腿，无需求驱动。

## Impact

- Affected specs: 触碰四处冻结面，已由 **D-025** 授权并各追加 Errata——`06`（E-06-002/E-06-003）、`07`（E-07-001）、`08`（E-08-001）；`11` 新增 §2.7 路由表、§3.2 事件类型、§5 `STORE_*` 与 `SKILL_*` 错误码前缀。
- Affected code:
  - 新增 `crates/forge-store/`（Cargo.toml + 7 个模块 + 单测）
  - 新增 `crates/mcp/store-mcp/`（Cargo.toml + main.rs + mcp.rs）
  - 新增 `crates/forge-util/src/hashutil.rs`；`crates/forge-util/{Cargo.toml,src/lib.rs}` 微调
  - 新增 `crates/forge-agentd/src/{skills.rs,store.rs}`
  - 修改 `crates/forge-agentd/src/{main.rs,agent.rs,llm.rs,mcp.rs,engine/mod.rs,native_tools.rs}`
  - 修改根 `Cargo.toml`（members +2）
  - 修改 `packages/host/src/plugins/forgeProxy.ts`（`PROXY_PREFIXES` +1、`isLongLivedPath` +1）
  - 新增 `packages/client/src/lib/{storeStore.ts,skillStore.ts}`、`packages/client/src/components/workbench/{StoreTab.tsx,SkillsTab.tsx}`
  - 修改 `packages/client/src/components/shell/{Sidebar.tsx,Workbench.tsx}`、`packages/client/src/lib/{workbenchStore.ts,commands.ts,forgeApi.ts}`、`packages/client/src/components/chat/Composer.tsx`、`packages/client/src/components/settings/SkillsPage.tsx`
  - 新增 `registry/`（官方源种子：index.json + packages/ + blobs/）
  - 新增 `scripts/f11-w1-store-smoke.ps1`、`scripts/f11-w2-skill-smoke.ps1`、`tools/e2e/f11-store-journey.mjs`；修改 `tools/e2e/run-all.mjs`
- 验证门禁：`cargo test --workspace` / `pnpm -r typecheck` / `pnpm -r test` / `go -C gateway-go test ./...` 全绿 + 两个新冒烟 exit 0 零孤儿进程 + `f11-store-journey` 矩阵逐任务布尔 verdict
- 前置依赖：无新增外部服务；官方源为仓内目录，离线可验收；`https` 社区源腿在无网络环境标注 `DEV_ENV_DEGRADE` 不充绿

## ADDED Requirements

### Requirement: Registry 协议与双源驱动

系统 SHALL 定义 `RegistrySource` 抽象并提供 `file://` 与 `https://` 两种驱动，二者对上层呈现同一套 `index / search / detail / manifest / blob / publish` 接口；源配置持久化于 `data/store-sources.json`，私有源 token 只存 keystore 不落配置文件。

#### Scenario: 官方文件源端到端

- **WHEN** 以仓内 `registry/` 为 `file://` 源执行 search → detail → manifest → blob
- **THEN** 逐步返回真实数据，blob 字节的 sha256 与 manifest 中登记值一致

#### Scenario: 源不可达如实上报

- **WHEN** 多源聚合搜索中某个源不可达
- **THEN** 该源以 `(sourceId, STORE_SOURCE_UNREACHABLE)` 进入返回的 `errors` 列表且不影响其他源的结果，UI 如实展示失败源而非伪造空列表

### Requirement: 安装走既有构建链且 provenance 强制

系统 SHALL 使商店安装的资产走与人工导入完全相同的 `asset_import` 构建链，落地后 `.meta.provenance.origin` 为 `store-install`，`detail` 含 `sourceId / packageId / packageVersion / fileSha256 / license / publisher / installedAt`。

#### Scenario: 安装后元数据可核

- **WHEN** 从官方源安装一个 asset-pack
- **THEN** `Content/` 下出现资产文件与配套 `.meta`，`provenance.origin == "store-install"` 且 `detail.packageId` 与所装包一致，`installed.json` 有对应记录

#### Scenario: 校验失败不留残留

- **WHEN** 下载到的字节 sha256 与 manifest 登记值不符
- **THEN** 返回 `STORE_CHECKSUM_MISMATCH` 且 `Content/` 下无任何该包的残留文件与 `.meta`

#### Scenario: 付费包诚实拒绝

- **WHEN** 安装 `pricing.amount > 0` 的包
- **THEN** 返回 `STORE_PAYMENT_REQUIRED`，UI 显示「暂不支持付费获取」诚实禁用态而非伪造购买流程

### Requirement: 包内路径禁锢

系统 SHALL 对包清单中每条文件路径施加路径安全校验（拒绝 `..`、前导 `/` 或 `\`、含 `:` 的盘符形态、空路径段），落地范围禁锢于项目 `Content/` 与 `skills/` 之内。

#### Scenario: 穿越路径被拒

- **WHEN** 包清单含 `../../etc/passwd` 或 `C:/Windows/x.dll` 形态的路径
- **THEN** `manifest.validate()` 返回 `STORE_MANIFEST_INVALID`，安装不启动

### Requirement: 卸载与技能删除经 Proposal 门

系统 SHALL 将商店卸载与技能删除判为 destructive，无 approved Proposal 时返回 409 `GOV_PROPOSAL_REQUIRED` 并自动创建 pending 提案；批准后同一调用放行。full-auto 模式不豁免。

#### Scenario: 首次卸载被拦并建单

- **WHEN** 未经批准直接调用卸载
- **THEN** 返回 409 `GOV_PROPOSAL_REQUIRED` 携 `proposalId`，`/api/forge/proposals` 中可见对应 pending 条目

#### Scenario: 批准后放行

- **WHEN** 该提案 `PATCH {action:"approve"}` 后重发同一卸载请求
- **THEN** 真实执行；被其他资产引用的条目如实进 `blocked_by_refs` 而非强删

### Requirement: 技能索引注入系统提示

系统 SHALL 在装配系统提示时注入「可用技能」索引（仅启用技能的 name 与 description），禁用技能不出现在索引中。

#### Scenario: 索引随启停变化

- **WHEN** 某技能经 `skills/config/write` 禁用后发起一轮对话
- **THEN** 系统提示的技能索引段中不含该技能

### Requirement: read_skill 原生工具

系统 SHALL 提供 `read_skill(name)` 原生工具，返回对应 SKILL.md 全文；工具出现在非 ask 模式的工具面中；技能不存在或被禁用时返回显式失败而非空内容。

#### Scenario: 工具在工具面中可见

- **WHEN** build 模式发起一轮对话
- **THEN** 下发给 provider 的 tools 数组含 `read_skill` 且其 schema 有必填 `name` 参数

#### Scenario: 禁用技能诚实拒绝

- **WHEN** 对已禁用的技能调用 `read_skill`
- **THEN** 返回失败并说明技能已禁用，不返回内容

### Requirement: 选中技能全文经 preamble 注入

系统 SHALL 使 `ask:execute` 接受结构化 `skills[]` 字段，将命中技能的全文作为独立 system 消息注入（与 F10 检索上下文共存，技能段在前），并发 `agent.skills.injected` 事件记录命中与缺失名单及注入字符数；前端不再使用 `Use skills:` 文本前缀。

#### Scenario: 全文真实进入上下文

- **WHEN** 带 `skills:["asset-cleanup"]` 发起一轮对话
- **THEN** 下发给 provider 的 messages 中存在一条 system 消息包含该 SKILL.md 的正文片段，且事件序中有 `agent.skills.injected`

#### Scenario: 缺失技能不静默丢弃

- **WHEN** `skills[]` 中含不存在的技能名
- **THEN** 该名字出现在 `agent.skills.injected` 事件的 `missing` 字段中

#### Scenario: 前缀协议下线

- **WHEN** 全仓检索 `Use skills:`
- **THEN** 零命中（前端改发结构化字段）

### Requirement: 技能生命周期管理

系统 SHALL 提供技能的新建、读取、覆写、删除、校验五类操作；新建缺省内容为符合 `06 §1` 格式契约的骨架模板；写入前校验 frontmatter 与正文三节，不合格返回 `SKILL_FRONTMATTER_INVALID` 或 `SKILL_BODY_INCOMPLETE`。

#### Scenario: 新建即合法

- **WHEN** 不带 content 新建技能
- **THEN** 生成的 SKILL.md 通过自身的 validate 校验（frontmatter 齐全、正文含执行流程/输出约束/失败回退三节）

#### Scenario: 既有技能向后兼容

- **WHEN** 用新解析器解析仓内既有 13 篇两键 SKILL.md
- **THEN** 全部解析成功，`version` / `license` / `tags` / `allowedTools` 为缺省值

### Requirement: 个人资产库内容寻址

系统 SHALL 提供跨项目的个人资产库，以 sha256 内容寻址存储 blob 并做去重；库与项目 `Content/` 经显式双向操作流转，不做自动同步。

#### Scenario: 同内容去重

- **WHEN** 以两个不同名称添加同一内容
- **THEN** blob 目录中只存在一份文件；移除其中一条后 blob 仍在，两条都移除后 blob 被清理

### Requirement: Sidebar 入口与 Workbench tab 承载

系统 SHALL 在 Sidebar 的 New Agent 按钮下方提供「资产商店」与「Skill 管理」两个导航按钮，点击打开对应 Workbench tab；不新增常驻面板，七区布局不变。

#### Scenario: 入口可达

- **WHEN** 点击 Sidebar 的两个新按钮
- **THEN** Workbench 分别激活 `store` 与 `skills` tab，tab 可关闭，重复点击不重复开 tab

## MODIFIED Requirements

### Requirement: skill frontmatter 格式契约

原 `06 §1`「front matter 仅 `name` + `description`」修改为：**必填** `name` + `description`，**可选** `version` / `license` / `tags` / `allowed-tools`；解析器对未知键宽容忽略。已由 Errata E-06-003 登记。

#### Scenario: 扩展键解析

- **WHEN** SKILL.md 的 frontmatter 含 `version: 1.0.0` 与 `tags: [a, b]`
- **THEN** 解析结果的 `version` 为 `"1.0.0"`、`tags` 为 `["a","b"]`，且不影响必填键校验

### Requirement: 设置页技能 tab 内容收窄

原 `07 §7.2` 的 skills tab 由「技能列表 + 启停」修改为「目录配置（extraDirs）+ 跳转 Skill 管理 tab」；列表与编辑归 Skill 管理 tab 独有，避免两处事实源。tab 存在性不变。已由 Errata E-07-001 与裁决 D-F11-E 登记。

#### Scenario: 单一事实源

- **WHEN** 在 Skill 管理 tab 中启停某技能后打开设置页
- **THEN** 设置页不再重复渲染技能列表，不存在两份可能不同步的状态

### Requirement: provenance origin 枚举

原 `08 §3.2` 的 `origin: user-import|gen-image|gen-model` 修改为增加 `store-install`。已由 Errata E-08-001 登记。

#### Scenario: 四值皆合法

- **WHEN** 读取商店安装资产的 `.meta`
- **THEN** `provenance.origin` 为 `store-install` 且被系统正常识别，不触发校验错误

## REMOVED Requirements

无（不移除既有门禁）。
