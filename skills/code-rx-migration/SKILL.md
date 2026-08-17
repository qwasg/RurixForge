---
name: code-rx-migration
description: .rx 代码修改与重构。当任务涉及「改脚本 / 重构 .rx / 修诊断」时使用。
---

# code-rx-migration · .rx 代码修改与重构

> SEAM 标注:依赖 code-forge 工具面(code_symbol_search / code_structured_edit / rx_check / rx_test,F4 承接,未落地),当前不可执行。

## 目标
对 .rx 脚本做定位精确的修改/重构,改后 rx_check 无新诊断、rx_test 全绿。

## 必须遵守
- 先定位后修改:code_symbol_search 定位符号,禁止全文盲改。
- 小步快跑:单次修改保持可编译,rx_check 逐步收敛。

## 分步骤执行流程(F4 工具面落地后生效)
1. code_symbol_search 定位目标符号与引用面。
2. code_structured_edit 结构化修改(不重排无关代码)。
3. rx_check:无新诊断(既有诊断不劣化)。
4. rx_test:相关测试全绿;CI 恒绿不破。
5. 报告:修改符号清单 + 诊断前后对比 + 测试结果。

## 输出约束
- 修改 diff 必须最小化;引入的新诊断必须清零或如实上报。

## 失败回退策略
- rx_check/rx_test 不过:逐 edit 回退到最近绿态,报告失败步骤与诊断详情。
