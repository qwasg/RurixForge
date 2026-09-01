---
contract: RD-DIGEST
title: open RD 消化波(14 项终态判定 + RD-F1-002 真 LLM 工具循环)
status: closed
implementation_status: unlocked
active_scope: done
version: 0.1
date: 2026-08-18
timebox: 会话制推进,做不完转 deferred
rfc_required: []
upstream_docs:
  - 12_SECURITY_PERMISSIONS.md (§5 R-5 密钥 keystore)
  - 06_SKILLS_LIBRARY.md (§3 标题笔误 errata)
  - 14_DECISION_LOG.md (终审追加面)
  - milestones/f0~f5 + rd-f4-004 各契约 deferred 段
implementation_unlock:
  required_all:
    - RD-F4-004 回填波 closed(commit ca2061f)
    - 用户开工指令(2026-08-18「三个全做」拍板 W1→W2→F6 + DeepSeek 官方 API key 提供)
in_scope:
  - **RD-F1-002 真 LLM 工具循环**:agentd llm.rs(provider 抽象:mock|deepseek;POST /api/forge/llm/chat;OpenAI tools 格式转换自 MCP tools/list 实测拉取;循环 max_iters;进程内 mcp::call_tool 执行;密钥 env FORGE_LLM_API_KEY → keystore["deepseek"];密钥不进日志/事件/返回);client sendChat 非 multitask 四模式换 F1 seam(scene_summary 回显 → /api/forge/llm/chat);scripts/rd-f1-002-llm-loop-smoke.ps1 live 冒烟(DeepSeek 真实调用,无 key 环境如实 SKIP 不充绿)
  - **RD-F5-001 keystore 加固(Windows 腿)**:gend keystore DPAPI(CryptProtectData 每用户熵)加密落盘,读回透明解密;env 优先语义不变;macOS/Linux 子项维持 OPEN
  - **RD-F3-001 errata**:06 §3 标题「首发 12 个」→ errata 追加(冻结文档只追加纪律)
  - **RD-F0-001 复核**:apps/ide 残余目录存在性实测(2026-08-18 仍在),本会话再尝试删除;锁则维持 OPEN
  - **重裁决终审**(证据驱动,14_DECISION_LOG 追加):RD-F0-002(上游 29 个里程碑闭合 tag 实测,非 release tag;path 依赖为开发期常态裁决)/ RD-F0-003(agentd 五 ServerKind 槽/75 工具/REST 面已成,七 crate 全量移植裁决 superseded——需求驱动补件)/ RD-F1-004(external semaphore no-go 终审 CLOSED)/ RD-F2-004(materials[] 需求未现,按需驱动终审 CLOSED)
  - **上游复核维持 OPEN**(重锚 refill 证据,不重做):RD-F2-001(rurix-asset 仅 gltf 实测)/ RD-F2-003(rurix-render 无 closure 公开面实测)/ RD-F4-001(rx fix|watch 仍 seam 实测)/ RD-F4-002(rurixc tooling 单文档会话复核)/ RD-F4-003(rurix-physics 无 sensor 实测)/ RD-F5-002(local-diffusers 工作量项重锚)
out_of_scope:
  - RD-F1-002 的流式输出(SSE)/多轮上下文记忆(首发单轮+工具循环;会话历史进 eventlog 不回注)
  - DeepSeek 之外的第二 provider(抽象留 seam)
  - keystore macOS Keychain/Linux secret-service(子项维持 OPEN)
deliverables:
  - id: D-RDG-1
    name: agentd llm.rs + /api/forge/llm/chat + client sendChat 换 seam + live 冒烟
    evidence: cargo test + client vitest + scripts/rd-f1-002-llm-loop-smoke.ps1 输出
  - id: D-RDG-2
    name: keystore DPAPI 加密腿 + 小项终审(errata/F0-001/重裁决/上游复核)
    evidence: cargo test(gend)+ 14_DECISION_LOG 追加 + 各契约 deferred 状态更新
acceptance_gates:
  - id: G-RDG-1
    name: 真循环门
    check: 配 key 环境:live 冒烟「创建 N 个立方体」经 DeepSeek 真实工具循环完成(toolCalls 非空 + scene_summary 实体数实增 + provider=deepseek);无 key 环境:provider=mock 如实标注,mock 恒绿门(agentd llm_complete_mock + host postMessage 双事件 + client sendChat 测试)不破;密钥全文扫描(冒烟日志/事件/返回)无泄漏
  - id: G-RDG-2
    name: 终审登记门
    check: 14 项 RD 全部有终态:每项 = CLOSED(闭环/裁决)/ OPEN(重锚 refill + 2026-08-18 复核证据);keystore DPAPI 往返测试过(密文文件不含明文子串);14_DECISION_LOG 终审条目追加;各源契约 deferred 段状态同步
guardrails:
  - 诚实优先:live 冒烟无 key/无网络如实 SKIP/FAIL,不伪造循环成功;裁决类终态必须有原文证据锚定
  - 密钥红线 R-5:DeepSeek key 只进 data/keystore.json(gitignore 覆盖)/env,永不落仓/契约/日志/事件
  - 数字必须来自命令输出;契约 §6 只追加
---

# RD-DIGEST 契约:open RD 消化波

## 1. 目标与双门状态

14 项 open RD 全部给终态判定(诚实优先:非全 CLOSED);RD-F1-002 以 DeepSeek 官方 API 真实闭环(用户 2026-08-18 提供 key,经 keystore/env 注入,不落仓)。status=active;implementation_status=unlocked。

## 2. 范围与波次

- wave.1:RD-F1-002(agentd llm.rs + 路由 + client seam + live 冒烟)→ G-RDG-1。
- wave.2:RD-F5-001 DPAPI + RD-F3-001 errata + RD-F0-001 复核 → D-RDG-2 前半。
- wave.3:重裁决终审 + 上游复核重锚 + 14_DECISION_LOG 追加 + 各契约 deferred 同步 + 全量回归 + close-out → G-RDG-2。

## 3. 架构决策(立项裁决)

- **D-RDG-A(LLM 循环落点 = agentd)**:工具面在 agentd 进程内(mcp::call_tool 前缀路由 stdio,长连接单例+断线重连);host/client 经 forgeProxy /api/forge/llm/* → 8103 既成。循环 = POST /api/forge/llm/chat { text, mode? } → tools/list 实测拉取五 server schema(懒加载+进程内缓存)→ OpenAI tools 格式 → DeepSeek chat.completions(tools, tool_choice=auto)→ tool_calls 逐项进程内 call_tool → role:tool 回注 → 终止(no tool_calls 或 max_iters=16)→ { provider, text, toolCalls[{name, ok, summary≤200ch}], iters }。
- **D-RDG-B(密钥面)**:FORGE_LLM_API_KEY env 优先 → gend keystore["deepseek"](key_for 语义,FORGE_GEN_API_KEY 共享 dev-key 覆盖如实标注);皆无 → provider=mock(mock 恒绿门不破,CI 无 key 全绿)。key 永不进日志/事件/工具返回(请求头组装点单测扫描)。
- **D-RDG-C(client seam 切换)**:sendChat 非 multitask 四模式:F1 seam(scene_summary 回显)→ POST /api/forge/llm/chat(forgeProxy 已代理);assistant 文本 = llm.text + toolCalls 摘要;mock provider 时如实显示 mock 标注;multitask 确定性执行器(D-F3-E)不动。
- **D-RDG-D(DPAPI 形态)**:keystore.json 值域整体 DPAPI(CryptProtectData,每用户熵,无额外熵盐)→ base64;文件形态 {"v":1,"dpapi":"<b64>"} 加密态 / 旧明文形态读回兼容迁移(读→加密重写);windows-sys 依赖(Win32_Security_Cryptography);macOS/Linux 编译期 fallback 明文 + 如实注释(子项 OPEN)。
- **D-RDG-E(终审纪律)**:裁决类 CLOSED 必须附「原文 reason + 2026-08-18 复核证据 + 裁决逻辑」三件套进 14_DECISION_LOG;上游维持项重锚 refill(实测命令+日期);不为了「消化率」虚闭任何一项。

## 4. Deferred 处置
本波新增 deferred 追加于下方。

(暂无)

## 5. 修订
- 2026-08-18 立项:RD-F4-004 回填 closed(ca2061f)后用户拍板 W2。勘探实测:agentd mock seam = llm_complete GET 占位 + client sendChat F1 seam(scene_summary 回显);host sessions postMessage echo seam(双事件测试不验文本,换 seam 不破);工具 schema 源 = 各 MCP server tools/list(agentd 无本地 schema 副本,KNOWN_TOOLS 仅名);rurix 29 tag 全里程碑闭合类非 release;06 §3 标题 L34 原文在库。

## 6. Close-out(只追加区)

### wave.1 验收记录(2026-08-18,RD-F1-002 真 LLM 工具循环)→ G-RDG-1 PASS
- 交付:crates/forge-agentd/src/llm.rs(provider 抽象 mock|deepseek;POST /api/forge/llm/chat;to_openai_tools;run_deepseek_loop max_iters=16;spawn_blocking ureq HTTPS;role:tool 回注;summary≤200ch;耗尽如实标注);mcp.rs list_all_tools(五 server tools/list 实测拉取+前缀+进程内缓存,nextCursor 如实报错);client editorStore.sendChat 非 multitask 四模式 F1 seam(scene_summary 回显)→ /api/forge/llm/chat,mock 如实 [mock] 标注,deepseek 渲染工具摘要,有成功调用才 reload+scene_summary;scripts/rd-f1-002-llm-loop-smoke.ps1(live/mock 双轨)。
- 测试数字(命令输出实测):cargo test -p forge-agentd **44/44**(新增 llm::tests×4 + 路由级 llm_chat_mock_provider_when_no_key / llm_chat_empty_text_400);pnpm --filter @forge/client test **75/75**(sendChat 三用例改写/新增:四模式载荷+mock 标注 / deepseek 工具摘要渲染+刷新 / 无调用不刷新);pnpm --filter @forge/client typecheck 绿;pnpm --filter @forge/host test **19/19**(postMessage 双事件门不破)。
- live 冒烟(evidence/rd-f1-002-llm-loop-smoke-*.log 实测):密钥面 keystore["deepseek"](gitignore 覆盖确认,data/keystore.json 不落仓)→ provider=deepseek,**iters=4,toolCalls=10**:DeepSeek 自主 component_list_types 探 schema → entity_create ×3(Cube1/2/3)→ component_add 缺 material 字段真实报错 ×3 → 自纠错重试 ×3 全 OK;scene_summary 实体数 **0 → 3 实增**;R-5 红线扫描(响应/日志全文)无密钥子串;exit 0。**G-RDG-1 live 轨 PASS**;mock 轨(无 key 环境)由 agentd/client/host 单测恒绿覆盖。
- 密钥红线 R-5 执行:key 仅经 keystore.json(env 优先语义保留)注入;请求体组装点单测 request_body_never_contains_key;Provider Debug 脱敏 Deepseek(<redacted>);上行错误消息只带 HTTP 状态码+上行 error.message,不回显请求体/头。
- 遗留(转 wave.3 收尾):全量回归(cargo workspace / pnpm -r / go)与 deferred 段同步在 wave.3 close-out 前统一跑;本波未动 host sessions postMessage echo seam(契约范围即不动,client 已绕开直达 llm/chat)。

### wave.2 验收记录(2026-08-18,RD-F5-001 DPAPI + RD-F3-001 errata + RD-F0-001 复核)
- **RD-F5-001 → CLOSED(Windows 腿)**:gend keystore DPAPI 加固落地。落盘形态 {"v":1,"dpapi":"<base64(CryptProtectData 密文)>"}(每用户熵,无额外熵盐,D-RDG-D);旧明文 {"keys":{...}} 读回兼容 + 透明迁移(读→加密重写);macOS/Linux 编译期明文 fallback(子项维持 OPEN)。测试:cargo test -p gend **25/25**(新增 dpapi_roundtrip / set_key_writes_encrypted_form_and_reads_back(密文文件无明文子串断言)/ legacy_plaintext_migrates_to_encrypted)。**实测迁移**:data/keystore.json(deepseek key)由 {"keys":...} 自动重写为 {"dpapi":"AQAAANCMnd8BF...}(标准 DPAPI blob 头),迁移后 rd-f1-002 live 冒烟复跑仍 PASS(密钥解析链 env→DPAPI keystore 完好)。windows crate LocalFree 0.61 签名 Option<HLOCAL> 踩坑已修。
- **RD-F3-001 → CLOSED**:06_SKILLS_LIBRARY.md §3 标题「首发 12 个」笔误,Errata(只追加区)E-06-001 追加——表实列 13 行,与 skills/ 目录 13 个 SKILL.md 实测一致,正确表述「首发 13 个」。
- **RD-F0-001 → CLOSED**:apps/ide 残余目录 2026-08-18 复核:LS apps/ 仅 desktop/(ide 目录磁盘已不存在,前会话 Trae 重启解锁后补删成功);git status --porcelain apps/ 无未跟踪残余;git ls-files apps/ 仅 apps/desktop/*。无需再删,终态闭合。

### wave.3 验收记录(2026-08-18,重裁决终审 + 上游复核重锚 + 全量回归)→ G-RDG-2 PASS
- **重裁决终审(4 项,三件套进 14_DECISION_LOG)**:
  - RD-F0-002 → CLOSED(D-016):path 依赖为 F0–F6 开发期常态,D-003 tag 锚定推迟至打包期。实测 `git -C H:\rurix tag -l` 29 tag = 25 里程碑闭合 + 4 版本 tag(v0.1.0-m8.4/v1.0.0/v1.0.1-dist.1/v1.0.1-dist.2),HEAD 8c5dc5ee(2026-08-18)。
  - RD-F0-003 → CLOSED(D-017 superseded):agentd 五 ServerKind/75 工具/REST 面需求驱动自成,七 crate 全量移植作废;04 功能面余项转路线图立项。
  - RD-F1-004 → CLOSED(D-018):external semaphore no-go 终局(单向语义 × 帧流向矛盾;重开条件登记)。
  - RD-F2-004 → CLOSED(D-019):materials[] 需求未现,单 material 注册表维持(需求驱动终态)。
- **上游复核维持 OPEN(6 项,重锚 2026-08-18 实测)**:
  - RD-F2-001:rurix-asset 网格导入 = gltf 唯一(cook.rs TOOL_GLTF_IMPORT;纹理侧 BC/DDS 在库,无 fbx/obj)。
  - RD-F2-003:**阻塞解除**——rurix-render 已补公开 material closure 面(material/mod.rs pub mod closure/MaterialParams/MaterialTable::closures()/side_table closure_32b_layout_digest,G9.5 RFC-0025);refill 转可执行(closure id 字段进 .rxmat + 缓存键),owner 下一材质/渲染波。
  - RD-F4-001:rx 子命令 = build/check/run/fmt/bench/vendor/test/**doc**(doc 已 M8.6 落地);fix|watch 仍 seam(main.rs L55–58)。
  - RD-F4-002:LSP capabilities 五件在,references_at 单文档 file_id 求值(lsp.rs L120/L146/L435),跨文件仍待上游。
  - RD-F4-003:rurix-physics 无 sensor(grep 仅命中注释),AABB 沿检测维持。
  - RD-F5-002:双仓无 diffusers/sd.cpp 痕迹,从零接入为大型工作量项。
- **各源契约 deferred 段同步(只追加)**:F0(三项全终态)/ F1(RD-F1-002、RD-F1-004 CLOSED)/ F2(两项 OPEN 重锚 + RD-F2-004 CLOSED)/ F3(RD-F3-001 CLOSED)/ F4(RD-F4-004 CLOSED 登记 + 三项 OPEN 重锚)/ F5(RD-F5-001 CLOSED Windows 腿 + RD-F5-002 重锚)。
- **全量回归(2026-08-18 命令输出)**:cargo test --workspace **201/201**(含修复 gen_configure_writes_config_and_keystore_redline 断言——F5 期「文件含明文密钥」断言与 DPAPI 直接冲突,改为平台条件断言:Windows dpapi 形态无明文 + Keystore 解密读回);pnpm -r typecheck 全绿;pnpm -r test **99/99**(protocol 5 + client 75 + host 19);go build + test forge-gateway ok。
- **14 项终态总账**:CLOSED 8(RD-F0-001/F0-002/F0-003/F1-002/F1-004/F2-004/F3-001/F5-001[Windows 腿;macOS/Linux 子项 OPEN])/ OPEN 重锚 6(RD-F2-001/F2-003[阻塞解除]/F4-001/F4-002/F4-003/F5-002)——无虚闭,每项附实测证据。
- **G-RDG-2 终审登记门:PASS**;G-RDG-1(wave.1)PASS。**RD-DIGEST 双门全绿,status flip:active → closed。**
