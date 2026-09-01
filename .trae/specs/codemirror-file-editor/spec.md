# CodeMirror 6 文件编辑器 Spec（依赖登记 + 差异留痕）

## Why

F8 wave.1 落地的工作区文件预览（`FilePreviewTab`）是纯只读 `<pre>` + 手写行号 gutter，正文零着色。本波需求为「文档预览加代码高亮 + 支持用户编写代码」——高亮与编辑是同一需求的两半，纯高亮方案（shiki）只解决一半。

## What Changes

- **agentd 写端点**：`PUT /api/forge/workspace/file`（workspace.rs `save_file_in`）——confine 复用只读面严格口径 `resolve_confined_file_in`（只改已存在文件，不新建）；写前双闸（>256KB 413 `FILE_TOO_LARGE` / 含 NUL 415 `BINARY_FILE`）；`baseModifiedAt` 乐观并发（陈旧 409 `FILE_CONFLICT`，缺省跳过 = 最后写入胜）；同目录 tmp + rename 原子落盘。GET 响应新增 `modifiedAt`（纳秒级 RFC3339 冲突令牌，比 tree 的秒级精）。
- **client 编辑器**：`FilePreviewTab` 改造为 CodeMirror 6 可编辑器（行号/折叠/查找/括号匹配/undo 栈/Ctrl+S 落盘），语言包按扩展名动态 import 懒加载；高亮配色经 `HighlightStyle` 全量绑定 `var(--code-*)` CSS 变量（theme.css 双表），主题切换零重建自动跟随。
- **EOL 保真**：CM6 内部归一 LF，加载时嗅探 CRLF、保存前还原，Windows 文件不被静默改 EOL。
- **tab dirty**：`WorkbenchTab.dirty` + tabbar 圆点 + 关闭 dirty tab 内联确认条（保存/放弃/取消）。

## 依赖登记（技术栈红线豁免申报）

F7 契约 guardrails「禁引入新框架（库级依赖允许，登记制）」。本波登记 12 个库级依赖（非框架，均为 CodeMirror 6 官方包）：

- 内核：`@codemirror/state` `@codemirror/view` `@codemirror/language` `@codemirror/commands` `@codemirror/search` `@codemirror/autocomplete` `@lezer/highlight`
- 语言：`@codemirror/lang-javascript` `@codemirror/lang-rust` `@codemirror/lang-json` `@codemirror/lang-markdown` `@codemirror/legacy-modes`

## 差异留痕：为何引 CodeMirror 而非 shiki

F7 契约 L150 留档「shiki 不引——代码块高亮评估成本>收益（两主题色对齐 + 懒加载 + jsdom 测试基建三项成本）」。本波裁决**引 CodeMirror 6、维持 shiki 不引**，理由：

1. **需求含编辑**：shiki 是纯高亮器，无编辑面；CM6 高亮/编辑/行号/查找/undo 一体，引一套解决整条需求。
2. **两主题色对齐**：CM6 的 HighlightStyle 支持 CSS `var()` 值，配色直接挂 `--code-*` 双表，主题切换由浏览器解析、编辑器零重建——当初 shiki 的双主题成本在 CM6 方案下不存在。
3. **jsdom 测试基建**：CM6 在 jsdom 下 DOM 渲染正常，仅需在 test/setup.ts 补 `Range.prototype.getBoundingClientRect/getClientRects` 空几何桩（无 WASM/ESM 加载问题）。
4. chat 气泡内 MarkdownFlat 代码块**维持纯样式渲染不接高亮**（F7 留档口径不变，本波不扩散）。

## Impact

- Affected code:
  - `crates/forge-agentd/src/workspace.rs`（+PUT handler + save_file_in + mtime_token + 5 单测）、`crates/forge-agentd/src/main.rs`（路由 +put）
  - `packages/client/package.json`（+12 依赖）
  - `packages/client/src/lib/forgeApi.ts`（apiPut + apiWorkspaceFileWrite + modifiedAt）
  - 新增 `packages/client/src/lib/cmLang.ts`、`cmTheme.ts`、`components/workbench/CodeEditor.tsx`
  - `FilePreviewTab.tsx`（只读预览 → 编辑器）、`workbenchStore.ts`（tab dirty）、`Workbench.tsx`（dirty 点）、`Shell.tsx`（Ctrl+S preventDefault）、`styles/theme.css`（--code-* 双表）
  - `packages/client/test/setup.ts`（Range 桩）、新增 `test/fileEditor.test.tsx`
- 明确不做：新建文件/重命名/删除（无入口 UI，需更宽松 confine 口径）；LSP/诊断/跳转定义；高亮色接入 `applyPalette` 自定义派色（跟 `data-theme` 切静态双表，theme.css 头注已留痕）。
- 验证门禁：`cargo test -p forge-agentd` + `pnpm --filter @forge/client test` + `pnpm -r typecheck` 全绿。
