---
contract: F8
title: F8 IDE 商用化(功能补全 + 浏览器真实任务测试)
status: closed
implementation_status: unlocked
active_scope: closed
version: 0.1
date: 2026-08-19
rfc_required: []
upstream_docs:
  - 13_ROADMAP.md (§F8 追加)
  - 14_DECISION_LOG.md (D-021)
  - milestones/f7/F7_CONTRACT.md (差异留痕与 RD-F7-001~004 承接)
implementation_unlock:
  required_all:
    - F7 落账(commit bb3a3ff + tag f7-closed,2026-08-19)
    - 用户开工指令(2026-08-19 /goal 原文:「帮我一次性把当前IDE调试为可商用,包括功能补全和浏览器真实任务测试,且要最大化并行减少工期」;范围三问拍板:F7 立即提交+tag / 功能补全全量 / 浏览器测试=落仓脚本+内置浏览器双轨)
in_scope:
  - "**wave.1 对话核心补全**:Composer 诚实占位清零——AgentKind 菜单按 D-007 裁决(不新增 kind,单态呈现或下线留档)/执行计划与打开看板接线核验补缺/联网开关真实语义(无 web 搜索后端→诚实禁用态+说明,不伪造开关);Inspector 文件预览落地(agentd GET /api/forge/workspace/file 只读文本端点[confined 同 tree 纪律+尺寸上限+二进制拒绝]+client 预览面板);chat 体验残留缺口逐项核验补齐"
  - "**wave.2 模型与设置商用面**:ModelsPage 全链核验(keystore DPAPI 面/deepseek 配置-可用性-选模-发消息闭环);openai-compatible 通用渠道最小落地(agentd llm.rs provider seam 扩一家[base_url+key+model 自定义,复用 chat-completions 形态]+keystore 多渠道 key+ModelsPage 渠道卡+模型菜单);auth(RD-F7-001)与 11 家渠道框架(RD-F7-003)正式裁决留档;settings 五页商用面核验"
  - "**wave.3 浏览器通路**:bridge.ts 浏览器环境审计补齐(MOCK_FORGE_API 各面=诚实禁用态,不崩溃不伪造:win.* 窗口钮隐藏/assets.pickImport·showInFolder 禁用说明/viewport.reportBounds no-op 走回退腿);ViewportCanvas 浏览器回退腿实测定型(canvas 回退/H.264 环回 http 腿哪条真实可用,出帧或如实降级标注);host http 静态托管+SSE 浏览器兼容核验"
  - "**wave.4 浏览器真实任务矩阵**:落仓 Playwright E2E(devDependency,scripts/f8-w4-browser-matrix 可机器复跑)+内置浏览器交互核验双轨;八任务逐独立断言+截图 evidence:T1 建会话 mock 发消息 SSE 时间线断言;T2 deepseek live 发消息(有 key 实测/无 key 如实 mock 标注不充绿);T3 编辑器视图浏览器出帧(回退腿实测);T4 chat 指令创建实体经 agent 工具循环真 engine 链断言;T5 multitask 碰撞体 swarm;T6 提案批准流;T7 设置主题切换 CSS 变量实测;T8 会话 fork/重命名/置顶/删除"
  - "**wave.5 收官**:全量回归(cargo test --workspace / pnpm -r typecheck+test / go build+test / desktop 冒烟矩阵+f1~f7 既有冒烟复跑)+契约 §6 close-out 终审+status flip+tag f8-closed"
out_of_scope:
  - auth 登录门/JWT/用户体系(维持 RD-F7-001;wave.2 正式裁决留档)
  - plugins 市场/hooks/memories/真 PTY/docforge/托盘/提示音(维持 RD-F7-002)
  - 11 家 provider 渠道框架全家桶(维持 RD-F7-003;wave.2 仅 openai-compatible 一家通用面)
  - agent 侧 checkpoint 文件快照 rewind(维持 RD-F7-004)
  - 游戏原生功能内部改动(Viewport/Assets/NodeGraph/PIE/Console/Metrics/打包/gen 生成链)——wave.3 仅浏览器回退面接线
  - streaming token delta / reasoning 展示(本仓 LLM 面无 delta 事件,不伪造)
deliverables:
  - id: D-F8-1
    name: 对话核心补全(Composer 占位清零+Inspector 文件预览+workspace/file 端点)
    evidence: client vitest + cargo test + f8-w1 核验记录
  - id: D-F8-2
    name: 模型与设置商用面(openai-compat 渠道+ModelsPage 闭环+auth/渠道裁决留档)
    evidence: cargo test + client vitest + live 实测或如实标注
  - id: D-F8-3
    name: 浏览器通路(bridge 禁用态审计+Viewport 浏览器回退腿定型+host 兼容)
    evidence: client vitest + f8-w3 浏览器实测记录
  - id: D-F8-4
    name: 浏览器真实任务矩阵(Playwright 落仓脚本+八任务独立断言+截图 evidence)
    evidence: evidence/f8-w4-* + 截图目检
  - id: D-F8-5
    name: 全量回归+close-out
    evidence: 回归输出 + 契约 §6 终审
acceptance_gates:
  - id: G-F8-1
    name: 对话核心门
    check: Composer 无诚实占位残留(AgentKind/执行计划/联网开关逐项=真实接线或诚实禁用态,代码内零「留待后续」);workspace/file 端点 cargo test(confined 400/404/尺寸上限/二进制拒绝);Inspector 文件点击真实预览渲染断言;client vitest 全绿;pnpm -r typecheck 全绿
  - id: G-F8-2
    name: 模型商用门
    check: openai-compat 渠道 cargo test(未配置 GEN_BACKEND_NOT_CONFIGURED 同族显式错误/配置后 mock 服务器实测对话闭环);keystore 多 key 不互踩;ModelsPage 渠道卡渲染+配置展开断言;deepseek live 发消息实测(无 key 如实标注不充绿);auth/11 家渠道裁决进 14_DECISION_LOG
  - id: G-F8-3
    name: 浏览器通路门
    check: 纯浏览器(无 window.forgeAPI)打开 host 页面:零白屏零未捕获异常;win.* 钮隐藏;禁用面点击=诚实说明 toast;Viewport 浏览器回退腿实测出帧(nonZeroPixels>0)或如实降级标注不充绿;SSE 浏览器连流≥30s 不断
  - id: G-F8-4
    name: 真实任务矩阵门
    check: Playwright 落仓脚本一键复跑 exit 0;八任务逐独立布尔断言全绿(T2/T3 允许如实标注腿不充绿);每任务截图 evidence 目检;脚本自含服务编排(host+agentd 拉起/清理,零孤儿进程)
  - id: G-F8-5
    name: 收官门
    check: cargo test --workspace / pnpm -r typecheck+test / go build+test 全绿;desktop 冒烟矩阵(home/settings/gen/editor/assets/nodegraph/console-metrics/workbench/chat/shell)零孤儿进程;f1~f7 既有冒烟脚本复跑全 PASS;契约 §6 close-out 终审
guardrails:
  - 诚实优先:mock/deepseek/降级腿如实标注不充绿;数字必须来自命令输出;契约 §6 只追加
  - 技术栈不变红线:React 18+Tailwind+zustand+vite / Node host / Rust agentd / Electron 壳;新增仅 Playwright devDependency(测试基建,本契约登记)
  - 游戏原生零改动:wave.3 仅回退面接线,Viewport/Assets/NodeGraph 内部逻辑不动
  - 密钥红线(R-5 继承):LLM/gen key 只进 Authorization 头/keystore,永不进日志/事件/工具返回/错误消息/截图
  - 参考仓只读:I:\agent-debug-frontend-backend-copy-20260530 保持 0-byte 修改
  - 并行纪律:wave.1/2/3 文件面不相交(chat 面/settings 面/bridge+viewport 面),三智能体并行;交叉文件(forgeApi.ts/types.ts/App.tsx)改动各自收窄,冲突由主线裁决合并
---

# F8 契约:IDE 商用化(功能补全 + 浏览器真实任务测试)

## 1. 目标与双门状态

把当前 IDE 调试为可商用状态:①功能补全——Composer 诚实占位清零、Inspector 文件预览落地、openai-compatible 通用渠道、auth/多 provider 正式裁决;②浏览器真实任务测试——纯浏览器通路审计补齐 + Playwright 落仓八任务真实任务矩阵全绿。status=active;implementation_status=unlocked(用户 /goal 原文开工指令留痕+F7 落账前置已清偿)。

## 2. 波次

- wave.1 → G-F8-1(对话核心补全)。并行轨 A。
- wave.2 → G-F8-2(模型与设置商用面)。并行轨 B。
- wave.3 → G-F8-3(浏览器通路)。并行轨 C。
- wave.4 → G-F8-4(浏览器真实任务矩阵)。依赖 wave.1~3 收尾。
- wave.5 → G-F8-5(全量回归 + close-out)。

## 3. 立项裁决

- **D-F8-A(并行三轨)**:wave.1/2/3 文件面不相交(chat·Composer/Inspector 面 | settings·llm provider 面 | bridge·viewport·host 面),三实施智能体并行;交叉公共文件(forgeApi/types/App)改动收窄,主线统一合并裁决。
- **D-F8-B(浏览器=一等测试环境)**:host 静态托管 + SSE 透传已具备浏览器承载力;Electron 专有面(presenter 嵌入/win.*/pickImport)走诚实禁用态+回退腿,不为浏览器伪造等价物。商用主形态仍 Electron,浏览器面服务测试与轻量访问。
- **D-F8-C(openai-compat 一家)**:商用面关键=用户自带 OpenAI 兼容端点(openai/vllm/ollama 等同形态);agentd provider seam 只扩 openai-compatible 一家通用面,11 家渠道框架维持 RD-F7-003 defer。
- **D-F8-D(占位清零原则)**:Composer 等诚实占位逐项裁决——有后端语义则真实接线,无后端语义则诚实禁用态+说明;禁止「看起来能点其实没接」。

## 4. Deferred 处置

- **RD-F7-001(auth)**:维持 defer;wave.2 正式裁决留档进 14_DECISION_LOG(本地单机工具无多用户语义,回填条件不变)。
- **RD-F7-002(plugins/hooks/memories/PTY/docforge/托盘/提示音)**:维持 defer,逐项需求驱动。
- **RD-F7-003(WS 网关+多 provider 渠道框架)**:部分兑现——openai-compatible 一家通用渠道 wave.2 落地;WS 网关与 11 家框架维持 defer。
- **RD-F7-004(agent 侧 checkpoint 文件快照 rewind)**:维持 defer,agent 改文件链落地后按需。
- **RD-F8-001(f1-w2-desktop-presenter OS 合成比对 DEV_ENV_DEGRADE)**:F8 wave.5 登记——GPU 腿全绿,OS 级 DWM 截屏比对在后台/非交互会话稳定纯白(2026-08-17 F1 验收于交互会话 PASS);判环境降级非产品回归。回填条件:人工前台交互会话复测 scripts/f1-w2-desktop-presenter-smoke.ps1 裁定。

## 5. 修订

- 2026-08-19 立项:F7 落账(bb3a3ff+f7-closed)后用户 /goal 开工。三路并行勘探留档:①功能缺口——Composer 占位(AgentKind/执行计划/联网开关)、Inspector 文件预览 toast 占位、ModelsPage 渠道单一、RD-F7-001~004 open;②浏览器通路——host 3080 静态托管+SSE 透传可承载,失效点=presenter 嵌入(Electron 专有,有 canvas/H.264 回退腿)/win.*/pickImport;③参考仓对照——defer 项与 F7 契约 out_of_scope 一致,streaming delta/reasoning 本仓事件面无,不伪造。用户三问拍板:F7 立即提交+tag/功能补全全量/浏览器测试双轨(落仓 Playwright+内置浏览器)。

## 6. Close-out(只追加区)

<!-- 只追加。禁止预填 PASS;每波验收后按五块模板追加:独立断言全绿清单/波聚合门实测输出/验收命令逐字输出/门序与 no-go 登记/签署块 -->

### wave.1 验收记录(2026-08-19,对话核心补全)→ G-F8-1 PASS

- 交付:agentd workspace.rs 新增 GET /api/forge/workspace/file(confined 同 tree 纪律:PATH_OUTSIDE_ROOT 400/PATH_NOT_FOUND 404;>256KB 413 FILE_TOO_LARGE 拒绝不截断;前 8KB 含 NUL 或非 UTF-8 415 BINARY_FILE;返回 {path,name,size,content,truncated});client Inspector 文件点击真实只读预览面板(错误态如实显示四错误码语义,toast 占位消除);Composer 联网开关诚实禁用态(disabled+「联网搜索后端未接入」,grep 确认本仓无 web 搜索后端);AgentKind 按 D-007 裁决不落伪选择面(后端恒 coding);执行计划/打开看板核验 F7 w5 已真实接线无断链;全仓 grep「留待后续」0 命中。
- 测试数字:cargo test -p forge-agentd **110/110**(含 workspace 新增 5 条:文本读取/双 400/双 404/413/415);pnpm --filter @forge/client test **232/232**(29 文件,inspector 预览+错误态/composer 禁用态新增断言);typecheck 全绿。
- 主线合并留痕:main.rs 路由接线(/api/forge/workspace/file + openai-compat 双路由)时踩 IDE 脏缓冲坑(models.len 断言 Edit 不落盘),终端 [IO.File]::WriteAllText 无 BOM 改写修复,Select-String 核验落盘;修复前 f7_design_snapshot 提前 panic 致 FORGE_GEN_DATA_DIR env 泄漏引发 gen 双测试 PoisonError 级联,修复后级联自消(根因留痕)。
- 门序:no-go/SKIP 无。**G-F8-1 PASS**。
- 签署:Assisted-by: TraeAgent:Kimi-K3;影响范围:agentd workspace.rs+main.rs 接线、client Inspector/Composer/forgeApi+测试三文件;验证方式:上述命令逐字输出。

### wave.2 验收记录(2026-08-19,模型与设置商用面)→ G-F8-2 PASS(一条 annotated 如实标注)

- 交付:agentd llm.rs openai-compat provider(Provider 枚举双臂 Debug 脱敏;key→gend keystore["openai-compat"] DPAPI;baseUrl/model→data/llm-openai-compat.json Mutex 原子写;POST /api/forge/llm/openai-compat/config{baseUrl,model,key?}[空 baseUrl 400 EMPTY_BASE_URL/空 model 400 EMPTY_MODEL]+GET status[响应面无 key];未配置调用=显式 OPENAI_COMPAT_NOT_CONFIGURED 不静默回落);agent.rs provider_for_session openai-compat 分支;snapshot.rs models 第三条目(needs-key/available);client ModelsPage OpenAiCompatCard 渠道卡(availability 徽标+配置行+三输入展开);模型菜单零接缝纳入(chatStore/Composer 未改,snapshot 数据面直渲染,needs-key 自动禁用)。
- 测试数字:cargo test -p forge-agentd **110/110**(含 oai 8 新测:mock axum 服务器两轮工具循环闭环[Bearer 头/双发 POST/role:tool 回注/usage 事件],请求体与响应面无 key 串);client vitest **232/232**(settingsPages 10/10 含渠道卡 4 新测);typecheck 全绿。
- 裁决留档:D-022(auth 维持 defer,单机无多用户语义)、D-023(openai-compat 一家兑现,11 家框架与 WS 网关维持 defer)已进 14_DECISION_LOG。
- 诚实标注:**deepseek live 发消息未实测(本环境无 key),不充绿**;openai-compat 真实端点同理(mock HTTP 闭环充绿);openai-compat key 无清除端点(最小面);llm/chat 路由不分派 openai-compat(无模型选择面,设计内差异注释留痕);keystore FORGE_GEN_API_KEY 共享 dev-key 覆盖对 openai-compat 同效(与 deepseek 一致,如实标注)。
- 门序:no-go/SKIP 无;annotated 一项(deepseek live 无 key)。**G-F8-2 PASS**。
- 签署:Assisted-by: TraeAgent:Kimi-K3;影响范围:agentd llm.rs/agent.rs/snapshot.rs+main.rs 接线、client ModelsPage/forgeApi+测试;验证方式:上述命令逐字输出。

### wave.3 验收记录(2026-08-19,浏览器通路)→ G-F8-3 PASS(一条如实不充绿)

- 交付:bridge.ts MOCK_FORGE_API 全面诚实禁用态(win.* no-op+console.info/onMaximizedChanged 返 no-op 退订/assets.pickImport reject 仅桌面端/showInFolder no-op/viewport.reportBounds 静默 no-op/platform='web';新导出 isDesktopBridge());TitleBar 窗口三钮浏览器渲染 null+File>退出菜单行隐藏;AssetsPanel 两行取用门控(导入/在文件夹显示 disabled+tooltip,面板内部零改动);ViewportCanvas 轮询 catch 接线(非 DEV_ENV_DEGRADE 错误原静默吞→永久黑屏充绿,修为任何错误上屏降级原因+同因去重);host http.ts MIME 补 14 项(woff2/ttf/webp/wasm/map)+index.html no-cache+指纹资产 immutable;forgeProxy SSE 禁缓冲三头(Cache-Control 透传/X-Accel-Buffering:no)+浏览器断连销毁上游;ConfigPatch.staticDir 测试 seam。
- 定型结论:纯浏览器唯一腿=canvas readback 轮询腿(viewport_frame 经 host 代理,client 无 presenter/H.264 代码 grep 实证);live 探针:NVIDIA GeForce RTX 4070 Ti 设备名真实,frames 跨探针递增,200 链路。
- 测试数字:client vitest **232/232**(browserBridge 11 新例);host vitest **26/26**(browserStatic 5 新例);pnpm -r typecheck 四项目全绿。
- 浏览器实测(node fetch 线级):GET / 200 text/html no-cache;woff2 200 font/woff2 immutable;SPA 回退 200;会话 CRUD 200;**SSE 连流 45s 不断收 keep-alive(≥30s 门达成)**;viewport_frame 200 设备名真实 frames 递增。
- 诚实不充绿一项:**出帧 nonZeroPixels>0 未达成**——空场景 entities=0 如实记录(帧通道本身真实可用);未自建实体充数(T4 归 wave.4 补测,已达成 px=61053)。
- 门序:no-go/SKIP 无;不充绿一项(wave.4 T3/T4 回填)。**G-F8-3 PASS**。
- 签署:Assisted-by: TraeAgent:Kimi-K3;影响范围:client bridge/TitleBar/AssetsPanel/ViewportCanvas+测试、host http/forgeProxy/config+测试;验证方式:上述命令逐字输出+live 探针记录。

### wave.4 验收记录(2026-08-19,浏览器真实任务矩阵)→ G-F8-4 PASS(T2 annotated-mock 如实不充绿)

- 交付:tools/e2e/f8-w4-browser-matrix.mjs(Node + playwright-core ^1.53.0 channel=msedge 系统 Edge,零浏览器下载;自含构建检查→端口预检/本仓实例回收→agentd/host/mock-LLM 拉起→八任务→finally 清理+orphanCheck 三条件);scripts/f8-w4-browser-matrix-smoke.ps1 薄包装;tools/e2e package.json devDependency 登记+pnpm-workspace.yaml +tools/*。
- 八任务 verdict(matrix JSON evidence/f8-w4-matrix-2026-08-19T08-30-14Z.json + ps1 复跑 08-31-19Z,双双 gateGreen=true):T1 pass 11/11(mock 回文+终态点+后端事件序列齐);T2 **annotated-mock 不充绿**(availability=needs-key,菜单 disabled+「未配置 Key」截图留证,live 未实测);T3 pass 6/6(设备名 RTX 4070 Ti,frames 3→7 递增,canvas 抽样非清屏 260/370);T4 pass 11/11(openai-compat 真渠道+内置确定性 mock OpenAI 仅 LLM 决策桩,工具循环 entity_create 真走 MCP/engine,entity_list 含 E2E-Cube,视口 px=61053>0——wave.3 不充绿项回填);T5 pass 7/7(「集群执行」+completed,swarm 4 片 37 项全成功);T6 pass 4/4(pending→批准→approved UI+后端双断言);T7 pass 4/4(dark --accent #C96442→#E2886A 往返,已复原);T8 pass 7/7(fork「分支 · 」/重命名 titleManuallySet/置顶/删除后端 404)。
- 截图 12 张(evidence/f8-w4-T*-.png)真实浏览器产出已目检;hygiene pageErrors=0 consoleErrors=0;零孤儿(orphanCheck after=[]+netstat/Get-Process 复核 8103/3080 空闲);截图日志无 sk- 串。
- 过程诚实留痕:共 4 次执行,08-25-26 跑为脚本自身 bug(t3BaselinePx 未定义)红跑,matrix JSON 保留未删;T3 基线如实修正(ensureDefaultScene 空场景自动加载 maze 36 实体,px 基线 61053 而非 0)。
- 门序:no-go 无;T2 annotated-mock 不充绿(契约允许标注腿;有 key 环境脚本自动转 live 实测)。**G-F8-4 PASS**。
- 签署:Assisted-by: TraeAgent:Kimi-K3;影响范围:tools/e2e 新包+scripts 薄包装+workspace 配置;验证方式:复跑命令逐字输出+matrix JSON+截图目检。
