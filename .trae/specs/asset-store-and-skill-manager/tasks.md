# 资产商店与 Skill 管理 — Tasks

- [x] Task 0（立项）: `14_DECISION_LOG.md` 追加 D-025（插件市场划界 / 不新增常驻面板 / 扩 provenance.origin / 商店形态 / skill 注入实装，五项裁决 + 驳回项）；新建 `milestones/f11/F11_CONTRACT.md`（frontmatter 全字段 + 六节，§6 Close-out 留空不预填）；`11_API_CONTRACTS.md` 加 §2.7 路由表、§3.2 事件类型、§5 `STORE_*` 与 `SKILL_*` 两行；`06`/`07`/`08` 各追加 Errata（E-06-002、E-06-003、E-07-001、E-08-001）；本 spec 三件套落盘
- [x] Task 1（wave.1）: `crates/forge-util` 新增 `hashutil.rs`（`sha256_hex` + 流式 `Sha256Stream`，委托 `rurix_pkg::sha256`，消除第四份重复）；新建 `crates/forge-store` 七模块（manifest / source / registry / library / install / publish / skillpkg）+ `StoreError` 十二错误码 + 全模块单测；根 `Cargo.toml` members +1。留痕：路径安全校验与 checksum 拒绝路径必须有独立断言
- [x] Task 2（wave.2）: agentd 抽出 `src/skills.rs`（结构化 frontmatter 解析 + 校验 + 原子写 + `scan_skills`/`find_skill` 统一名字口径 + 七个 handler + `skills_index_prompt`/`skills_preamble`）；`main.rs` 删旧代码并注册五条路由；`engine/mod.rs` 与 `native_tools.rs` 三处注册 `read_skill`；`agent.rs` 系统提示索引注入 + `ask:execute` 结构化 `skills[]` 经 preamble 注入 + `agent.skills.injected` 事件。留痕：既有 13 篇 SKILL.md 向后兼容需有遍历断言；测试须自清理不污染 `skills/` 与 `data/skills-config.json`
- [x] Task 3（wave.3）: 新建 `crates/mcp/store-mcp`（照 context-mcp 模板；快操作八工具 + 异步 `store_install`/`store_uninstall`/`store_task_status`）；agentd 新增 `src/store.rs` REST 面（源 CRUD / 搜索 / 详情 / 安装长任务 / 卸载 / 任务进度 / 已装清单 / 更新检查 / 个人库 / 发布）；`mcp.rs` 六处注册 + `agent.rs` `WRITE_TOOLS` + 根 `Cargo.toml` members +1；`forgeProxy.ts` 加 `/api/forge/store` 前缀与 `isLongLivedPath` 豁免；卸载接 Proposal 门（照 `mcp_call` 的 `forced_asset_delete` 模板）
- [x] Task 4（wave.4）: `Sidebar.tsx` 加两个导航按钮；`workbenchStore.ts` 加 `store`/`skills` 两个 `BuiltinTabKind` 与 `BUILTIN_TABS` 条目；`Workbench.tsx` 加 `TAB_ICONS` 与分派；`commands.ts` 加两条命令；`forgeApi.ts` 加 store 端点封装；新建 `lib/storeStore.ts` 与 `components/workbench/StoreTab.tsx`（发现/已安装/我的资产库三子页 + 源管理 + 搜索过滤 + 卡片网格 + 详情页 + 安装进度 + 错误如实展示）；vitest 覆盖
- [x] Task 5（wave.5）: 新建 `lib/skillStore.ts` 与 `components/workbench/SkillsTab.tsx`（列表 + 启停 + 新建/编辑/删除 + 全文编辑器 + 校验提示 + extraDirs 配置 + 来源标记 + 删除走 Proposal 确认）；`Composer.tsx` 文本前缀下线改结构化字段 + 菜单按 `enabled` 过滤；`SkillsPage.tsx` 收敛为目录配置 + 跳转；vitest 覆盖
- [x] Task 6（wave.6）: 仓内 `registry/` 官方源种子（`index.json` + `packages/<id>/<ver>.json` + `blobs/<前2位>/<sha256>`，含 asset-pack 与 skill 两类真实示例包）；`scripts/f11-w1-store-smoke.ps1` 与 `scripts/f11-w2-skill-smoke.ps1`；`tools/e2e/f11-store-journey.mjs` 浏览器矩阵；`run-all.mjs` `STACK_SMOKES` 接入；evidence 落盘
- [x] Task 7（收官）: 全量回归（client/host/protocol vitest + typecheck + go test + 两冒烟 + journey 8/8）；`F11_CONTRACT.md` §6 六波 Close-out 五块记录；frontmatter `status: active → closed`。`cargo test --workspace` 因本机常驻 8103 锁 exe 未能整仓覆盖写（已 annotated 不充绿）。`git tag f11-closed` 待用户明确要求提交后打

# Task Dependencies

- [Task 1] [Task 2] depends on [Task 0]（立项裁决未落地不得动冻结面对应的代码；两者之间文件面不相交可并行——Task 1 只碰 `crates/forge-store` + `crates/forge-util` + 根 Cargo.toml，Task 2 只碰 `crates/forge-agentd` 的 skill 面）
- [Task 3] depends on [Task 1]（store-mcp 与 agentd store.rs 均内嵌 `forge-store` 库，库未成型无法编译）
- [Task 4] depends on [Task 3]（前端商店消费 store REST 面）
- [Task 5] depends on [Task 2]（前端 skill 消费新的 CRUD 与结构化 `skills[]` 字段）
- [Task 4] [Task 5] 之间文件面不相交可并行（StoreTab 面 vs SkillsTab 面；交叉的 `workbenchStore.ts`/`Sidebar.tsx`/`forgeApi.ts` 由 Task 4 统一改动，Task 5 只读不写这三个文件）
- [Task 6] depends on [Task 3] [Task 4] [Task 5]（冒烟与 journey 需要全链可跑）
- [Task 7] depends on [Task 6]
