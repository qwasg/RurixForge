---
name: skill-creator
description: 创建或修改 skill。当任务涉及「新建 skill / 改 skill / 沉淀操作流程」时使用。
---

# skill-creator · skill 创建/修改

> SEAM 标注:依赖 agentd fs 工具面(fs_write/read_skill 原生工具,RD-F0-003 七 crate 移植承接),当前不可由引擎内 agent 执行;人类/IDE 侧 agent 可直接按本规程手工落地。

## 目标
把可复用操作规程沉淀为 `skills/<name>/SKILL.md`,格式合法、可被 `skills/list` 发现。

## 必须遵守
- 格式契约(06 §1):front matter 仅 name + description;name = 小写英文+中划线;description 必须含触发时机。
- 正文必含:分步骤执行流程、输出约束、失败回退策略。
- 工具引用必须全名(`mcp__engine-scene__entity_batch_apply`),不得假设未列工具存在。
- skill 只读生效:落地后经 `GET /api/forge/skills/list` 验证可见即完成,无需重启(热加载)。

## 分步骤执行流程
1. 明确触发域与工具面:列出 skill 要用的全部工具,逐一核对已存在(`GET /api/forge/mcp/tools`);不存在的工具 → 正文首行 SEAM 标注(依赖 + 承接里程碑)。
2. 写 SKILL.md:front matter + 目标 + 必须遵守 + 分步骤执行流程 + 输出约束 + 失败回退策略。
3. 校验:`GET /api/forge/skills/list` 出现且 description 正确;`GET /api/forge/skills/{name}` 全文与磁盘一致。
4. 可执行性:依赖工具全存在 → 走一遍真实流程验证;有 seam → 如实标注不可执行。

## 输出约束
- 新 skill 必须经 list/read 双验证;SEAM 项必须在验收记录中如实声明,不伪装可执行。

## 失败回退策略
- 格式不合法(list 不出现)→ 检查 front matter 起止 `---` 与 name 合法性;仍失败报告并附文件内容。
