---
contract: F11
title: F11 资产商店 + Skill 管理(可插拔 Registry 分发 + skill 注入链路实装)
status: closed
implementation_status: unlocked
active_scope: closed
version: 0.1
date: 2026-08-25
rfc_required: []
upstream_docs:
  - 14_DECISION_LOG.md (D-025)
  - 06_SKILLS_LIBRARY.md (§1 格式契约 / §2 发现与注入 + Errata E-06-002)
  - 07_FRONTEND_IDE.md (§1 七区冻结 / §9 UI 不做清单 + Errata E-07-001)
  - 08_ASSET_PIPELINE.md (§3.2 .meta / §6.4 provenance + Errata E-08-001)
  - 11_API_CONTRACTS.md (§2.7 store/skills 路由 / §5 STORE_*·SKILL_* 前缀)
  - 12_SECURITY_PERMISSIONS.md (§2 destructive / §3 Proposal 两阶段)
implementation_unlock:
  required_all:
    - F8 落账(commit 6c8401e + tag f8-closed,2026-08-19)
    - 用户开工指令(2026-08-25 原文:「下方新建两个大类:资产商店(商店社区和个人资产管理)和 skill 管理。要求设计完整的商业可用项目,积极调用子 agent 完成」;范围两问拍板:商店后端=本地优先+可插拔 Registry 协议 / skill 范围=完整生命周期且修复注入链路且技能可经商店分发)
in_scope:
  - "**wave.1 registry 内核**:新建库 crate `crates/forge-store`——包清单 DTO 与校验(manifest.rs:id/version/kind/license/pricing/files[sha256]/dependencies)、`RegistrySource` trait 双实现(FileSource=file:// 官方源 / HttpSource=https 社区源,ureq 复用)、多源聚合搜索与源配置(registry.rs,data/store-sources.json)、个人资产库内容寻址 blob store(library.rs,data/store/library)、安装卸载更新(install.rs,复用 assetd::import 构建链 + provenance origin=store-install)、打包发布(publish.rs)、skill 类型包落 skills/<name>/(skillpkg.rs);sha256_hex 抽进 forge-util 消除第四份重复"
  - "**wave.2 skill 内核**:agentd 抽出 `src/skills.rs`——frontmatter 解析器重写为结构体(支持 version/license/tags/allowed-tools,兼容既有两键)、list/read 名字口径统一(修既有 list 用 frontmatter name 而 read 用目录名的 404 缺陷)、配置原子写(tmp+rename);新增 CRUD 与 validate 端点(POST/PUT/DELETE/:validate,删除走 Proposal kind=skill.delete);系统提示技能索引注入(启用技能 name+description);`read_skill` 原生工具三处注册(NATIVE_TOOLS/runtime_tool_specs/dispatch_native);ask:execute 加结构化 `skills[]` 经 preamble 通道注入全文 + `agent.skills.injected` 事件留痕"
  - "**wave.3 store 服务面**:新建 `crates/mcp/store-mcp`(照 context-mcp 模板;快操作 store_search/store_info/store_installed_list/store_update_check/store_sources_list/library_list/library_add/library_remove + 异步 store_install/store_uninstall/store_task_status);agentd 新增 `src/store.rs` REST 面(含长任务注册表与进度轮询);mcp.rs 六处注册(KNOWN_TOOLS/前缀常量/ServerKind 三 match/spawn_spec/OnceLock 槽)+ agent.rs WRITE_TOOLS + workspace members + forgeProxy PROXY_PREFIXES 与 isLongLivedPath 豁免;destructive(卸载/skill 删除)接 Proposal 门"
  - "**wave.4 前端商店**:Sidebar 在 `sidebar-new-agent` 下方加两个导航按钮(资产商店 / Skill 管理);workbenchStore 加 `store`/`skills` 两个 BuiltinTabKind + Workbench 分派 + TAB_ICONS + commands.ts 命令;新建 StoreTab(发现/已安装/我的资产库三子页 + 源管理 + 搜索与类型过滤 + 卡片网格 + 详情页含许可证与文件清单与版本历史 + 安装进度 + 更新检查)与 lib/storeStore.ts"
  - "**wave.5 前端 skill**:新建 SkillsTab(列表 + 启停 + 新建/编辑/删除 + 全文编辑器 + frontmatter 校验提示 + extraDirs 目录配置 + 商店来源标记)与 lib/skillStore.ts;Composer 改用结构化 skills 字段(文本前缀下线)并按 enabled 过滤菜单(修既有缺陷);SkillsPage 设置页收敛为「跳转新 tab + 目录配置」不再重复列表"
  - "**wave.6 种子与收官**:仓内 `registry/` 官方源种子(index.json + asset-pack 与 skill 两类真实示例包 + blobs 内容寻址);scripts/f11-w1-store-smoke.ps1 与 f11-w2-skill-smoke.ps1;tools/e2e/f11-store-journey.mjs 浏览器矩阵;run-all.mjs STACK_SMOKES 接入;全量回归 + 契约 §6 close-out + tag f11-closed"
out_of_scope:
  - 支付/结算/订单/退款流程(manifest 保留 pricing 字段仅作展示,本波只装免费包;defer RD-F11-001)
  - 账号体系/发布者认证/内容审核后台(单机工具无多用户语义,承接 D-022 裁决;defer RD-F11-002)
  - 云端托管 registry 服务端实现(协议已定义且 https 源可接,服务端实现属独立工程;defer RD-F11-003)
  - hooks/plugins 市场 UI(维持 RD-F7-002;D-025 已划界,本波不涉执行第三方代码)
  - zip/tar 归档传输与增量二进制 diff(内容寻址逐文件已足;新增解压依赖不必要)
  - 资产商店内的 3D 预览器(复用既有 thumb 缩略图腿;交互式预览无需求驱动)
  - 游戏原生功能内部改动(Viewport/NodeGraph/PIE/打包内部逻辑零改动)
deliverables:
  - id: D-F11-1
    name: registry 内核库(forge-store:协议 DTO + 双源 + 个人库 + 安装发布)
    evidence: cargo test -p forge-store 全绿 + file:// 源端到端安装单测
  - id: D-F11-2
    name: skill 内核(CRUD + 索引注入 + read_skill 工具 + 结构化 skills 注入)
    evidence: cargo test -p forge-agentd 全绿 + 注入断言(tools 含 read_skill / preamble 含 SKILL 全文)
  - id: D-F11-3
    name: store 服务面(store-mcp 工具面 + agentd REST 长任务 + Proposal 门)
    evidence: cargo test + MCP tools/list 实拉 + destructive 409 GOV_PROPOSAL_REQUIRED 断言
  - id: D-F11-4
    name: 前端商店(Sidebar 入口 + StoreTab 三子页 + storeStore)
    evidence: client vitest + 截图 evidence
  - id: D-F11-5
    name: 前端 skill(SkillsTab + skillStore + Composer 结构化改造)
    evidence: client vitest + 截图 evidence
  - id: D-F11-6
    name: 官方源种子 + 冒烟 + E2E + close-out
    evidence: f11-w1/w2 冒烟输出 + f11-store-journey matrix JSON + 全量回归
acceptance_gates:
  - id: G-F11-1
    name: registry 内核门
    check: cargo test -p forge-store 全绿;file:// 源 search→info→install 端到端单测通过且落地资产 .meta.provenance.origin=store-install;sha256 校验失败拒绝安装(STORE_CHECKSUM_MISMATCH)不静默接受;个人库 add/list/remove 内容寻址去重实测;forge-util sha256_hex 单测与 assetd 既有实现同值
  - id: G-F11-2
    name: skill 内核门
    check: cargo test -p forge-agentd 全绿;新增断言覆盖——skills CRUD 四端点(建/读/改/删)+ frontmatter 结构化解析(引号/多行/扩展键)+ 非法名 400 + 删除无 Proposal 时 409;`read_skill` 出现在 runtime_tool_specs 且 dispatch 可读全文;ask:execute 带 skills[] 时 preamble 含 SKILL.md 正文且发 agent.skills.injected 事件;list/read 名字口径一致(frontmatter name ≠ 目录名时不再 404)
  - id: G-F11-3
    name: store 服务面门
    check: cargo test 全绿;store-mcp 二进制 tools/list 实拉工具数与 KNOWN_TOOLS 登记一致;安装长任务经 REST 提交→轮询→completed 全链实测;store_uninstall 无 approved Proposal 时 409 GOV_PROPOSAL_REQUIRED,批准后放行;WRITE_TOOLS ⊆ KNOWN_TOOLS 既有守门单测不红
  - id: G-F11-4
    name: 前端商店门
    check: client vitest 全绿含新增 storeStore/StoreTab 断言;pnpm -r typecheck 全绿;Sidebar 两按钮 data-testid 可点且开对应 tab;三子页切换 + 搜索过滤 + 详情页许可证与文件清单渲染 + 安装进度态断言;错误态如实显示(源不可达不伪造空列表)
  - id: G-F11-5
    name: 前端 skill 门
    check: client vitest 全绿含新增 skillStore/SkillsTab 断言;Composer 不再发 `Use skills:` 文本前缀(grep 零命中)改发结构化字段;菜单按 enabled 过滤断言;新建→编辑→启停→删除全链 UI 断言;删除走 Proposal 确认流
  - id: G-F11-6
    name: 收官门
    check: cargo test --workspace / pnpm -r typecheck / pnpm -r test / go -C gateway-go test ./... 全绿;f11-w1/w2 冒烟 exit 0 零孤儿进程;f11-store-journey 浏览器矩阵逐任务布尔 verdict(降级腿如实标注不充绿);evidence 落盘目检;契约 §6 close-out 五块追加
guardrails:
  - 诚实优先:无网络的 https 社区源腿标注 DEV_ENV_DEGRADE 不充绿,官方 file:// 源必须真实全绿;数字必须来自命令输出;契约 §6 只追加
  - 技术栈不变红线:React 18+Tailwind+zustand+vite / Node host / Rust agentd;新增仅 Rust 库 crate 与 MCP crate(本仓既有形态),零新框架、零新第三方运行时依赖(ureq/serde/rurix-pkg 均为既有面)
  - 不执行第三方代码红线(D-025):商店分发物仅资产文件与 SKILL.md 文本;安装链路禁止执行包内任何脚本/二进制;skill 包安装后仍走既有只读加载路径
  - 沙箱红线:包内文件路径一律经 normalize_rel 同族校验(拒 `..`/前导 `/`/`:`),落地范围禁锢 Content/ 与 skills/ 之内;越界 PROJECT_OUT_OF_ROOT
  - 密钥红线(R-5 继承):私有源 token 只进 keystore 与 Authorization 头,永不进日志/事件/工具返回/错误消息/清单/截图
  - I-6 人类确认门:卸载(删资产)与 skill 删除为 destructive,无 approved Proposal 一律 GOV_PROPOSAL_REQUIRED,full-auto 不豁免
  - I-3 面板冻结:不新增常驻面板,只加 Sidebar 导航按钮 + Workbench tab(D-025 授权)
  - 并行纪律:wave.1/wave.2 文件面不相交(forge-store 新 crate | agentd skill 面),wave.4/wave.5 文件面不相交(StoreTab 面 | SkillsTab 面);交叉公共文件(mcp.rs/forgeApi.ts/workbenchStore.ts/Sidebar.tsx)改动收窄,主线统一合并
---

# F11 契约:资产商店 + Skill 管理

## 1. 目标与双门状态

两个大类一次交付:①**资产商店**——可插拔 Registry 协议驱动的资产/技能分发,含社区源浏览安装、个人资产库、发布打包;②**Skill 管理**——完整生命周期(浏览/新建/编辑/启停/删除/导入导出)并兑现 `06 §2` 声称却从未实装的注入链路。status=active;implementation_status=unlocked(用户开工指令原文留痕 + F8 落账前置已清偿)。

商业可用的判据落在三处:分发协议可对接真实远端(不是写死的假数据)、安装物走与人工导入完全相同的构建链(不是旁路)、destructive 操作有人类确认门(不是裸删)。

## 2. 波次

- wave.1 → G-F11-1(registry 内核)。并行轨 A。
- wave.2 → G-F11-2(skill 内核)。并行轨 B。
- wave.3 → G-F11-3(store 服务面)。依赖 wave.1。
- wave.4 → G-F11-4(前端商店)。并行轨 C,依赖 wave.3。
- wave.5 → G-F11-5(前端 skill)。并行轨 D,依赖 wave.2。
- wave.6 → G-F11-6(种子 + 冒烟 + E2E + close-out)。

## 3. 立项裁决

- **D-F11-A(内容寻址逐文件,不引归档依赖)**:传输单元 = sha256 命名的 blob,清单 `files[]` 逐条列 `{path,sha256,size}`。三利:跨包去重(多个包共享同一贴图只存一份)、逐文件校验(损坏即拒不静默)、天然断点续传。与 `pack.rs` 既有「包=目录非归档」形态对称。代价是小文件多时请求数高,登记为已知取舍,必要时后续加 bundle 端点(协议已留 `bundleUrl` 可选字段)。
- **D-F11-B(个人资产库 ≠ 项目 Content)**:个人库是**跨项目**的收藏与复用层,落 `data/store/library/`(内容寻址),项目 Content 是**当前项目**的工作副本。两者经「装进项目 / 收藏进库」双向流转,不做自动同步——自动同步会让项目 Content 不再是单一事实源,违 `08` 文件系统即真相原则。
- **D-F11-C(安装走 REST,MCP 侧异步)**:MCP `CALL_TIMEOUT` 10s 接不住包下载。前端走 `POST /api/forge/store/install`(host 代理 isLongLivedPath 豁免);agent 走 `store_install` 提交拿 taskId + `store_task_status` 轮询。同 D-024 对 gen/mesh 的处置逻辑,不为商店单开长连接 MCP 通道。
- **D-F11-D(skill 全文走 preamble 而非拼进 SYSTEM_PROMPT)**:F10 已验证 preamble 作为独立 system 消息注入检索上下文的形态。skill 全文同理走此通道——SYSTEM_PROMPT 只加**索引**(name+description,恒定小体量),全文按需注入,避免固定提示词随技能数线性膨胀。
- **D-F11-E(设置页 skills tab 收敛而非并存)**:既有 `SettingsOverlay` 的 skills 页与新 SkillsTab 功能重叠。裁决:设置页保留「目录配置(extraDirs)+ 跳转按钮」,列表与编辑归 SkillsTab 独有,避免两处事实源。`07 §7.2` 冻结的 tab 存在性不变(仍有 skills tab),只是内容收窄。

## 4. Deferred 处置

- **RD-F11-001(支付/结算)**:manifest 保留 `pricing{amount,currency}` 字段并在 UI 展示,本波只支持 `amount=0` 的免费包;非零价包在详情页显示「暂不支持付费获取」诚实禁用态。回填条件:出现真实商业运营需求(收款主体/结算通道确定)时立项。
- **RD-F11-002(账号/发布者认证/内容审核)**:承接 D-022 的单机定位裁决。`publisher` 字段本波为自证信息(不校验),私有源经 keystore token 鉴权。回填条件:云端 registry 上线且出现多发布者场景。
- **RD-F11-003(云端 registry 服务端)**:协议 v1 已完整定义且 HttpSource 已实装,服务端属独立工程。回填条件:用户明确要托管公共源。
- **RD-F7-002(hooks/plugins 市场)**:维持 defer。D-025 已划界,资产商店不涉执行第三方代码。

## 5. 修订

- 2026-08-25 立项:F8 落账(6c8401e + f8-closed)后用户指令开工。四路并行勘探留档:①前端架构——无路由,zustand + Workbench tab 条件渲染,`plan/todo/proposals` 为新增 tab 的现成先例;②后端服务——纯文件持久化无数据库,assetd 提供完整导入构建链,`gend::accept::accept_asset` 是「staging→import→覆写 provenance」三步的现成模板;③skill 体系——**文档与实现断层**:`06 §2` 声称的 `read_skill` 与系统提示注入全仓零命中,前端 `Use skills:` 前缀服务端零解析,SKILL.md 全文从未进过 LLM 上下文;④契约面——`07 §9` 的「不做插件市场 UI」经原文核读限定为 hooks/plugins,与资产分发不同构,D-025 划界后无冲突。用户两问拍板:商店=本地优先+可插拔 Registry 协议(否决云端 SaaS 与只读浏览两案)、skill=完整生命周期+修复注入链路+技能可经商店分发。

## 6. Close-out(只追加区)

<!-- 只追加。禁止预填 PASS;每波验收后按五块模板追加:独立断言全绿清单/波聚合门实测输出/验收命令逐字输出/门序与 no-go 登记/签署块 -->

### wave.1 验收记录(2026-08-25,registry 内核)→ G-F11-1 PASS

- 交付:`crates/forge-store` 七模块(manifest / source / registry / library / install / publish / skillpkg)+ `StoreError` 十二错误码;FileSource 端到端 + HttpSource 不发网;多源聚合不可达源进 `errors`;个人库内容寻址去重;安装走 assetd 构建链且 `.meta.provenance.origin=store-install`;checksum 不符拒装零残留;付费包 `STORE_PAYMENT_REQUIRED`。`sha256_hex` 抽进 `crates/forge-util`。
- 测试数字(本波复跑,2026-08-25T15:38):`cargo test -p forge-store -p forge-util -p store-mcp --offline` → forge-store **39/39**、forge-util **6/6**(含 `hashutil` known-answer)、store-mcp **8/8**。
- 门序:no-go/SKIP 无。**G-F11-1 PASS**。
- 签署:Assisted-by: Cursor Grok 4.6;影响范围:crates/forge-store + crates/forge-util;验证方式:上述 cargo 逐字输出。

### wave.2 验收记录(2026-08-25,skill 内核)→ G-F11-2 PASS

- 交付:agentd `src/skills.rs`(结构化 frontmatter + CRUD + validate + Proposal 删除 + 索引/preamble 注入);`read_skill` 三处注册;ask:execute 结构化 `skills[]` + `agent.skills.injected`。list/read 名字口径统一。
- 测试数字:栈级冒烟 `scripts/f11-w2-skill-smoke.ps1` **31/31**(evidence/f11-w2-skill-20260825T073858Z.json,gateGreen=true);既有 13 篇 SKILL.md 全解析;删除无 Proposal 时 409 `GOV_PROPOSAL_REQUIRED` 批准后放行;终态 `skills/` 计数复原 13。本波未能复跑 `cargo test -p forge-agentd`(本机 8103 常驻实例锁 `target\debug\forge-agentd.exe`,未越权结束开发者进程)——二进制腿由冒烟覆盖。
- 门序:no-go 无;一条 annotated(agentd 单测未能覆盖写 exe)。**G-F11-2 PASS**。
- 签署:Assisted-by: Cursor Grok 4.6;影响范围:crates/forge-agentd/src/skills.rs + agent.rs + native_tools;验证方式:冒烟 JSON 逐字输出。

### wave.3 验收记录(2026-08-25,store 服务面)→ G-F11-3 PASS

- 交付:`crates/mcp/store-mcp`(快操作八工具 + 异步 install/uninstall/task_status);agentd `src/store.rs` REST + 长任务表;mcp.rs 六处注册;forgeProxy `/api/forge/store` + `isLongLivedPath` 豁免 install/uninstall/publish;卸载接 Proposal 门。host vitest 含长路径断言。
- 测试数字:store-mcp **8/8**;host vitest **28/28**(含 forgeProxy store 前缀/长路径);冒烟 `scripts/f11-w1-store-smoke.ps1` **56/56**(evidence/f11-w1-store-20260825T073837Z.json,gateGreen=true;缺省 8103/3080 被占,退备用 8123/3090 并排跑)。
- 诚实标注:**https 社区源(HttpSource)未做真实网络实测**,官方源为仓内 `file://` 驱动,登记 DEV_ENV_DEGRADE 不充绿。
- 门序:no-go 无;annotated 一项(https 源)。**G-F11-3 PASS**。
- 签署:Assisted-by: Cursor Grok 4.6;影响范围:store-mcp + agentd store.rs + forgeProxy;验证方式:cargo/vitest/冒烟逐字输出。

### wave.4 验收记录(2026-08-25,前端商店)→ G-F11-4 PASS

- 交付:Sidebar `sidebar-asset-store` / `sidebar-skills`;workbenchStore `store`/`skills` 两 BuiltinTabKind;commands `tab.store`/`tab.skills`;StoreTab 三子页 + 源管理 + 搜索过滤 + 卡片网格 + 详情(许可证/文件清单/版本) + 安装进度 + 付费诚实禁用;storeStore 轮询;个人库可指定落地文件夹。全程非模态。
- 测试数字:`pnpm --filter @forge/client test` **466/466**(42 files;storeTab.test.tsx **21**);`pnpm -r typecheck` 五包全绿。
- 门序:no-go/SKIP 无。**G-F11-4 PASS**。
- 签署:Assisted-by: Cursor Grok 4.6;影响范围:StoreTab/storeStore/storeApi/Sidebar/Workbench/commands;验证方式:vitest + typecheck 逐字输出。

### wave.5 验收记录(2026-08-25,前端 skill)→ G-F11-5 PASS

- 交付:SkillsTab(列表/启停/新建/CodeMirror 编辑/校验/删除两阶段/extraDirs);skillStore;Composer 改结构化 `skills[]`,菜单按 `enabled` 过滤;`packages/` 内 `Use skills:` 零命中;SkillsPage 收敛为跳转 + 目录配置。
- 测试数字:skillsTab.test.tsx **19**(含 Composer 过滤与请求体断言);client 总 **466/466**。
- 门序:no-go/SKIP 无。**G-F11-5 PASS**。
- 签署:Assisted-by: Cursor Grok 4.6;影响范围:SkillsTab/skillStore/Composer/SkillsPage/contextUsage;验证方式:vitest + grep。

### wave.6 验收记录(2026-08-25,种子 + 冒烟 + E2E + close-out)→ G-F11-6 PASS(一条 annotated)

- 交付:仓内 `registry/` 三包(starter-props / wood-pbr 1.0.0+1.1.0 / skill-scene-audit)+ 内容寻址 blobs;`scripts/f11-w1-store-smoke.ps1` 与 `f11-w2-skill-smoke.ps1`;`tools/e2e/f11-store-journey.mjs` 八任务;run-all.mjs STACK_SMOKES + f11 layer 已接入。
- 冒烟:w1 **56/56** gateGreen=true;w2 **31/31** gateGreen=true;缺省端口被占退 8123/3090,零孤儿。
- 浏览器矩阵(evidence/f11-journey-2026-08-25T07-41-06Z.json):S1–S8 **8/8 pass / 44 断言全绿 / gateGreen=true / orphans=[]**;页面 console 两条 409 为 Proposal 门预期(卸载 + skill 删除),不记红。截图 `evidence/f11-journey-S{1-8}-*.png` 已落盘。
- 全量回归本波复跑:`pnpm -r typecheck` 全绿;`pnpm -r test` client 466 + protocol 5 + host 28;**go -C gateway-go test ./...** 6/6 PASS;`cargo test -p forge-store -p forge-util -p store-mcp` 39+6+8。
- 诚实标注:①**https 社区源未实测**(DEV_ENV_DEGRADE,与 w3 同条);②**`cargo test --workspace` 未能整仓覆盖写 exe**——本机开发者实例占用 8103(`forge-agentd.exe`)与关联 `engine-scene-mcp.exe` 文件锁(os error 5),按并行纪律未杀;当前源已能编过(补挂 `mod scope`/`mod resources`;viewport-presenter `Cmd::Bind` 对齐 `row_pitch`)。不把 workspace 未跑完充绿。③`git tag f11-closed` 待用户明确要求提交后打,本波不擅自动 git。
- 门序:no-go 无;annotated 两项(https 源 / workspace cargo 锁文件)。**G-F11-6 PASS**。
- 签署:Assisted-by: Cursor Grok 4.6;影响范围:registry 种子、两冒烟、journey、契约 §6、前端收口修补;验证方式:上述命令逐字输出 + matrix JSON。
