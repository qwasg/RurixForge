# 资产商店与 Skill 管理 — Checklist

## 立项与契约

- [x] `14_DECISION_LOG.md` 有 D-025，五项裁决齐全且「驳回项」列非空（列出云端 SaaS / 只读浏览 / zip 归档 / 新增常驻面板 / 维持文本前缀五个被否方案及原因）
- [x] `milestones/f11/F11_CONTRACT.md` frontmatter 全字段填齐，`implementation_unlock` 含前置里程碑落账与用户开工指令原文
- [x] `F11_CONTRACT.md` §6 Close-out 在验收前为空（无预填 PASS）；验收后已追加六波记录
- [x] `11_API_CONTRACTS.md` §5 的 `STORE_*` 与 `SKILL_*` 前缀与代码中实际抛出的错误码逐一对得上（无表外裸码）
- [x] `06`/`07`/`08` 三份冻结文档只追加 Errata，正文零改动（`git diff` 核验）

## wave.1 registry 内核（G-F11-1）

- [x] `cargo test -p forge-store -p forge-util` 全绿，测试数字来自命令输出（39 + 6）
- [x] `forge-util::hashutil::sha256_hex` 对 `"abc"` 返回标准值 `ba7816bf…20015ad`，与 `assetd`/`forge-index` 既有实现同值
- [x] `manifest.validate()` 对空 id / 大写 id / 坏版本 / 空 files / `../` 路径 / `C:/` 路径 / 短 sha256 / skill 包缺 SKILL.md 逐条返回正确错误码
- [x] FileSource 端到端（index → search → detail → manifest → blob → publish → 再 search 可见）单测通过
- [x] HttpSource 单测不发真实网络请求（无网环境可跑绿）
- [x] 多源聚合中不可达源进 `errors` 而非静默跳过，其他源结果不受影响
- [x] 个人库同内容去重实测：两条不同名同内容 → blobs 只一份；移除一条 blob 仍在，全移除后被清
- [x] 安装端到端：`.meta.provenance.origin == "store-install"` 且 `detail.packageId` 正确
- [x] checksum 不符时返回 `STORE_CHECKSUM_MISMATCH` 且 `Content/` 下**零残留**（断言目录为空）
- [x] `pricing.amount > 0` 返回 `STORE_PAYMENT_REQUIRED`
- [x] 重复安装 `STORE_ALREADY_INSTALLED`；`force=true` 可重装
- [x] 卸载后文件与 `.meta` 均删、db 记录消失；被引用项进 `blocked_by_refs` 未强删
- [x] 全部测试用临时目录，跑完 `git status` 无新增垃圾文件

## wave.2 skill 内核（G-F11-2）

- [ ] `cargo test -p forge-agentd` 本波未能覆盖写 exe（8103 常驻实例锁文件，已 annotated）；栈级冒烟 31/31 覆盖 CRUD/Proposal
- [x] 仓内既有 13 篇 SKILL.md 全部 `parse_frontmatter` 成功（冒烟遍历断言 13/13）
- [x] 带引号 / 行内数组 / 未知键 / `allowed-tools` 的样本解析正确
- [x] `read_skill` 出现在 build 模式下发的 tools 数组中（`scripted_step` 捕获断言）
- [x] 对禁用技能调用 `read_skill` 返回显式失败，不返回内容
- [x] `ask:execute` 带 `skills[]` 时 messages 中有 system 消息含 SKILL.md 正文片段
- [x] 事件序中有 `agent.skills.injected`，`missing` 字段记录未命中名
- [x] 不带 `skills[]` 时不发该事件、preamble 无技能段
- [x] 技能索引段随启停变化（禁用后不出现在系统提示中）
- [x] CRUD 四端点断言齐（建成功/重名 409/非法名 400/改成功/改不存在 404/校验合法与非法）
- [x] 删除无 approved Proposal 时 409 `GOV_PROPOSAL_REQUIRED` 且创建 pending 单；批准后放行
- [x] 新建缺省模板自身通过 validate（frontmatter 齐 + 正文三节齐）
- [x] 测试自清理：跑完 `skills/` 无新增/删除目录，`data/skills-config.json` 与测试前一致
- [x] 全仓 grep `parse_skill_frontmatter` 只在 checklist 命中（产品代码旧实现已删净）

## wave.3 store 服务面（G-F11-3）

- [ ] `cargo test -p forge-agentd` 同 wave.2，锁 exe 未复跑
- [x] `store-mcp` 二进制可 spawn，工具声明单测 8/8（十三工具含 schema）
- [x] 既有守门单测 `write_tools_subset_of_known_tools` 不红（新写工具已进 `WRITE_TOOLS`）
- [x] 安装长任务全链：REST 提交返 `taskId` → 轮询 running → completed，进度 phase 真实变化
- [x] `store_uninstall` 无 approved Proposal 时 409 `GOV_PROPOSAL_REQUIRED`，批准后放行
- [x] `forgeProxy.ts` 的 `PROXY_PREFIXES` 含 `/api/forge/store`，`isLongLivedPath` 豁免安装端点（host vitest 断言）
- [x] 源不可达时 REST 如实返回 `STORE_SOURCE_UNREACHABLE` 而非 500 或空列表

## wave.4 前端商店（G-F11-4）

- [x] `pnpm --filter @forge/client test` 全绿（466），`pnpm -r typecheck` 全绿
- [x] Sidebar 两个按钮 `data-testid` 可点，分别开 `store` / `skills` tab；重复点击不重复开
- [x] 三子页（发现 / 已安装 / 我的资产库）切换断言
- [x] 搜索与类型过滤断言；空结果显示空态而非骨架屏卡死
- [x] 详情页渲染许可证、发布者、版本列表、文件清单
- [x] 安装进度态渲染（phase + done/total），完成后列表刷新
- [x] 源不可达时页面如实显示失败源与原因，不伪造空列表
- [x] 付费包详情页显示「暂不支持付费获取」诚实禁用态，安装钮 disabled

## wave.5 前端 skill（G-F11-5）

- [x] `pnpm --filter @forge/client test` 全绿
- [x] 产品代码 `packages/` 内 `Use skills:` 零命中（文档/契约保留历史表述，不复活发送协议）
- [x] Composer 技能菜单按 `enabled` 过滤（禁用技能不出现）
- [x] Composer 发送时携带结构化 `skills` 字段（请求体断言）
- [x] SkillsTab 新建 → 编辑 → 启停 → 删除全链 UI 断言
- [x] 删除触发 Proposal 确认流（非模态，不用 `<dialog>`——F1 坑：模态会永久阻塞无人值守冒烟）
- [x] 校验失败时编辑器内联显示 errors/warnings，不静默保存
- [x] 设置页 skills tab 不再重复渲染技能列表

## wave.6 种子与收官（G-F11-6）

- [x] `registry/index.json` 与各 manifest 的 sha256 与 blobs 实际内容一致（脚本校验 7 blob / 0 mismatch）
- [x] 种子含 asset-pack 与 skill 两类各至少一个真实可安装的包
- [x] `scripts/f11-w1-store-smoke.ps1` exit 0（56/56）
- [x] `scripts/f11-w2-skill-smoke.ps1` exit 0（31/31）
- [x] `tools/e2e/f11-store-journey.mjs` 逐任务布尔 verdict，matrix JSON 落 `evidence/`（8/8，44 断言）
- [x] 结束后无编排器拉起的 8103/3080 孤儿进程（`orphanScan` 空；本机原有 8103/3080 开发者实例未动）
- [x] `run-all.mjs` 的 `STACK_SMOKES` 已接入两个新冒烟
- [ ] `cargo test --workspace` 因本机常驻实例锁 exe 未能整仓覆盖写，已 annotated 不充绿
- [x] `pnpm -r typecheck` 全绿
- [x] `pnpm -r test` 全绿（client 466 + protocol 5 + host 28）
- [x] `go -C gateway-go test ./...` 全绿（6/6）
- [x] 截图 evidence 目检（商店三子页 + 详情页 + Skill 管理 + 删除提案）
- [x] 无网络的 `https` 社区源腿标注 `DEV_ENV_DEGRADE`，未充绿
- [x] 失败项只登记，未改产品代码充绿
- [x] `F11_CONTRACT.md` §6 六波 Close-out 五块记录追加完毕，数字均来自命令输出
