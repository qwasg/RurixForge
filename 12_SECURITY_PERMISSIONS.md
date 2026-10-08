# 12 · 安全与权限

> 照搬 agent-cowork 的 permission / proposal / checkpoint 三件套,扩展到场景与资产域。
> 目标:agent 可以放手做,人类始终握有否决权与回滚能力(I-6,红线 R-3)。

## 1. 权限模式(per-session,照搬)

| 模式 | 语义 |
|---|---|
| `read-only` | 仅 read tier 工具;write/destructive 一律拒绝 |
| `ask`(默认) | write 按规则表自动放行或提案;destructive 一律提案 |
| `full-auto` | write 放行;destructive 仍提案(不可关闭,I-6) |

切换:`PUT /api/forge/sessions/{id}/permission-mode`;默认值在设置页 `permissions` tab。

## 2. 规则表

- 规则 = `{ toolPattern, tier, pathScope?, action: allow|deny|proposal }`;按序匹配,首个命中生效,兜底 = `proposal`(write)/ `deny`(destructive)。
- 工具元数据 tier 见 `05 §1.3`;destructive 清单(冻结):`entity_destroy`、`asset_delete(force)`、`scene_checkpoint_restore`、`git_checkout`、`git reset` 类、`fs_write` 覆盖 `.rxscene/.rxprefab` 全量重写。
- `run_command` 限制:工作目录限定项目根;命令白名单(`rx`、`git` 只读子命令、构建脚本);其他一律提案。
- API:`/api/forge/permissions/rules` 管理;变更记事件 `permission.*`。

## 3. Proposal(确认单)

- 触发:destructive 工具、规则表 `proposal` 命中、bulk 工具超阈值(`06 §4`:实体 >50 或资产 >20)。
- 内容(DTO 见 `11 §4`):操作摘要 + 影响面(实体数/资产清单/文件 diff 统计)+ 发起人(session + tool)。
- 流转:`pending` →(UI Proposal 卡片 / API PATCH)`approved|rejected`;rejected = 工具调用返回 `GOV_PROPOSAL_REQUIRED` 且副作用为零(先提案后执行,工具实现必须两阶段:dry-run 收集影响面 → 批准后真执行)。
- 超时策略:默认无超时,会话挂起等待;`full-auto` 也不豁免(R-3)。

## 4. Checkpoint 与回滚

### 4.1 三层快照

| 层 | 内容 | 触发 |
|---|---|---|
| 文件编辑快照 | agent-tools 写文件前自动复制(照搬 agentd checkpoint) | 每次 `fs_write`/`str_replace_edit`/`apply_patch` |
| 场景快照 | `.rxscene` 复制 + 实体/组件摘要元数据,存 `T_SCENE_SNAPSHOTS` | bulk 场景工具自动前置(`05 §2.4`);用户手动;`play_enter` 前 |
| 项目快照 | git commit(自动分支 `forge/snapshot/*`) | 用户手动 / `asset-cleanup` 等高风险 skill 起手 |

### 4.2 回滚

- `workspace/revert`:文件层(照搬)。
- `scene_checkpoint_restore`:场景层,engine-host 重载快照(I-4:磁盘事实源,恢复 = 文件还原 + host 重载)。
- git 层:经 project-mcp;`reset --hard` 类操作 destructive + Proposal。

## 5. 资产安全

- 删除防护:`asset_delete` 引用阻断默认(`08 §5.2`);force 删除 = destructive + Proposal 双门。
- 生成内容:provenance 强制(I-7);后端密钥 keystore 保管,不进前端/项目文件/日志/事件(R-5)。
- 项目根禁锢:`fs_*` 与工具 pathScope 限项目目录;越界 `PROJECT_OUT_OF_ROOT`。

## 6. 审计

- 全量事件日志(JSONL)+ 会话 replay(`11 §3`):任何 agent 操作可事后追溯「哪个会话哪个工具用什么参数改了什么」。
- 诊断导出(About tab):打包脱敏日志(密钥/token 模式掩码)供 issue 反馈。

## Errata(只追加区)

- **E-12-001(2026-09-26,D-041)——云端凭据与账号安全面**:①**R-5 延伸到云端凭据**:refresh token、access token、设备 API Key 只在 agentd(前两者分别在 keystore / 内存,设备 Key 在 keystore),不进前端、事件、日志与项目文件;唯一例外是用户在「账户 → API Key」里自建的 Key,创建响应明文返回一次(给外部工具用)。keystore 新增 `secret_for`(不受 `FORGE_GEN_API_KEY` 开发覆盖)与 `remove_key`。②**forge-cloud**:密码 argon2id;平台 Key 与 refresh token 仅存 SHA-256;上游凭据 AES-256-GCM(主密钥 `FORGE_CLOUD_MASTER_KEY`,丢失即无法解密);JWT 15 分钟 + refresh 轮换与复用检测(复用即吊销整个会话及其设备 Key);登录 / 兑换 / 验证码限速;管理写操作记审计日志且不含密钥明文;`FORGE_CLOUD_ENV=prod` 拒绝默认密钥启动;默认不落请求与响应正文。③**本机面不变**:agentd/host 仍只监听 127.0.0.1 且无鉴权,权限模式与 Proposal 门(§1–§3)照旧;云端只做账号与计费鉴权。④**合规**:以订阅账号(ChatGPT / Claude)对外售卖额度可能违反上游服务条款,有封号风险;对外商用流量建议以 API Key 账号为主(见 `15_CLOUD_SERVICE.md` §9)。
