# 06 · Skill 体系

> Skill = `skills/<name>/SKILL.md` 形式的可复用操作规程,agent 在任务匹配时经
> `read_skill` 工具读取全文后严格执行。机制照搬 agent-cowork(`agent-tools/skill.rs`
> + `skills/` 目录),本章定义格式契约与游戏域核心清单。

## 1. 格式契约(照搬 agent-cowork)

```markdown
---
name: scene-greybox
description: 白盒搭建关卡空间。当任务涉及「搭关卡 / 摆白盒 / blockout / 灰盒」时使用。
---

# 标题

## 目标 / 必须遵守 / 分步骤执行流程 / 输出约束 / 失败回退策略
```

规则(与上游一致,违者 `read_skill` 拒绝或 skill-creator 校验失败):

- name = 小写英文 + 中划线;目录 `skills/<name>/SKILL.md`。
- front matter 仅 `name` + `description`;description 必须含触发时机(供系统提示「可用技能」列表检索)。
- 正文必须含:分步骤执行流程、输出约束、失败回退策略。
- skill 内引用工具必须用全名(`mcp__engine-scene__entity_batch_apply`),不得假设未列工具存在。
- skill 只读;创建/修改经 `skill-creator` skill 或手工 PR,热加载(无需重启)。

## 2. 发现与注入(照搬)

- agentd 启动扫描 `skills/`(可配多目录),生成「可用技能」索引(名称 + description)注入系统提示。
- 任务匹配 → agent 调 `read_skill(name)` 取全文 → 按流程执行。
- 管理 API:`/api/forge/skills/list`、`/api/forge/skills/{name}`、`/api/forge/skills/config/write`(`11 §4`)。

## 3. 游戏域核心 skill 清单(首发 12 个)

| skill | 触发域 | 核心流程摘要 | 主要工具面 |
|---|---|---|---|
| `skill-creator` | 创建/修改 skill | 照搬 agent-cowork 同名 skill | fs_write / read_skill |
| `scene-greybox` | 关卡白盒搭建 | 读设计意图 → 规划分区 → `entity_batch_apply`/`transform_batch_set` 摆盒体 → 视口截图自检 → 列问题清单 | engine-scene |
| `scene-dressing` | 场景装饰摆放(植被/道具散布) | 采样区域(physics_overlap/cast_ray 落地)→ 按密度/规则批量实例化 → 截图抽检 | engine-scene |
| `prefab-workflow` | 预制体创建/覆盖/回写 | 选中集 → prefab 创建/实例化 → 覆盖检查 → Apply/Revert | engine-scene + project |
| `material-tuning` | 材质/灯光参数调整 | `render_get_settings` 基线 → 单变量改 → 截图对比 → 收敛或回滚 | engine-scene |
| `asset-import-batch` | 批量导入素材目录 | 扫描源 → 分类(网格/贴图/音频)→ 设定导入设置 → `asset_import` → 失败项报告 | asset-pipeline |
| `asset-cleanup` | 素材整理/引用修复 | `asset_refs` 建图 → 孤儿/重复检测 → 提案移动/删除(Proposal 门)→ `asset_fix_redirectors` | asset-pipeline |
| `gen-asset-fill` | AI 生成素材补缺 | 列缺口清单 → gen-image/gen-model 生成候选 → 用户挑拣 → `gen_accept` 入管线 | gen-image / gen-model |
| `logic-blueprint-gen` | 交互逻辑生成 | 需求 → 节点图 JSON(`10 §4`)→ `rx_check` 等价校验 → 挂载 ScriptComponent → playtest 验证 | code-forge + engine-scene |
| `code-rx-migration` | `.rx` 代码修改与重构 | `code_symbol_search` 定位 → `code_structured_edit` → `rx_check` → `rx_test` | code-forge |
| `playtest-regression` | 玩法回归测试 | 场景 × 断言矩阵 → `test_run`(swarm `test-matrix` 分片)→ 汇总失败 | playtest |
| `debug-scene-issue` | 场景问题诊断(debug 模式) | 收集:截图 + events_drain + graph_dump + 组件快照 → 假设排序 → 单变量验证 → 报告 | engine-scene |
| `perf-budget-check` | 性能预算核查 | 帧统计事件 → mesh_inspect 热点 → 渲染设置基线 → 优化提案 | engine-scene + asset-pipeline |

## 4. skill 编写规范(游戏域追加)

- **先查询后修改**:任何修改型 skill 的第一步必须是读取现状(场景 summary/资产列表/诊断),禁止盲改。
- **dryRun 优先**:批量操作必须先 `dryRun: true` 拿 preview,数量超阈值(默认 >50 实体或 >20 资产)必须升 Proposal(`12 §3`)。
- **可验证收尾**:每个修改型 skill 的最后一步 = 验证(截图对比 / `rx_check` 无新诊断 / playtest 断言),验证失败 = 报告而非掩盖。
- **不越域**:skill 声明的工具域之外的操作,必须返回用户/agent 主循环处理,skill 内不得「顺手做」。

## Errata(只追加区)

- **E-06-001(2026-08-18,RD-F3-001)**:§3 标题「首发 12 个」为笔误——§3 表实列 13 行(skill-creator / scene-greybox / scene-dressing / prefab-workflow / material-tuning / asset-import-batch / asset-cleanup / gen-asset-fill / logic-blueprint-gen / code-rx-migration / playtest-regression / debug-scene-issue / perf-budget-check),与仓内 `skills/` 目录 13 个 SKILL.md 实测一致。正确表述应为「首发 13 个」。标题原文不改(冻结纪律),以本条为准。
- **E-06-002(2026-08-25,F11 / D-025)**:§2「发现与注入」所述机制在 F11 之前**未实装**——`read_skill` 工具与系统提示技能索引注入在代码中零命中,实际链路是前端把选中技能名拼成 `Use skills: a, b.` 文本前缀而服务端零解析,SKILL.md 全文从未进入 LLM 上下文。F11 wave.2 兑现:①`read_skill` 落地为**原生工具**(非 MCP;进程内 `dispatch_native` 分发,禁锢工作区与 `extraDirs` 之内);②agentd 装配系统提示时注入「可用技能」索引(仅启用项的 name + description);③`ask:execute` 新增结构化 `skills[]` 字段,选中技能全文经 preamble 通道作独立 system 消息注入(不并进 SYSTEM_PROMPT,避免固定提示词随技能数线性膨胀,裁决见 D-F11-D),并发 `agent.skills.injected` 事件留痕。§2 原文描述自本条起为实况。
- **E-06-003(2026-08-25,F11 / D-025)**:§1 格式契约「front matter 仅 `name` + `description`」放宽为「**必填** `name` + `description`,**可选** `version` / `license` / `tags` / `allowed-tools`」。可选键为商店分发所需(版本比对、许可证展示、检索标签)与工具域声明;解析器对未知键宽容忽略,既有 13 篇两键 SKILL.md 无需改动。校验规则不变:缺必填键或正文缺「分步骤执行流程 / 输出约束 / 失败回退策略」三节 → `SKILL_FRONTMATTER_INVALID` / `SKILL_BODY_INCOMPLETE`。另:skill 自 F11 起可经资产商店分发(`kind: skill` 包,落 `skills/<name>/`),分发物仅 SKILL.md 文本,不含可执行代码(D-025 红线)。
