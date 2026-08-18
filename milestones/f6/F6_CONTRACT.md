---
contract: F6
title: F6 试玩回归与打包(13_ROADMAP 最终里程碑)
status: active
implementation_status: unlocked
active_scope: wave.5
version: 0.1
date: 2026-08-18
rfc_required: []
upstream_docs:
  - 13_ROADMAP.md (§F6 交付/验收门原文)
  - 08_ASSET_PIPELINE.md (§7 打包本期最小)
  - 04_AGENT_WORKFLOW.md (§5.3 多 headless host 并发)
  - skills/playtest-regression/SKILL.md (F3 seam,工具面本里程碑兑现)
implementation_unlock:
  required_all:
    - RD-DIGEST 消化波 closed(commit c6fcfb7)
    - 用户开工指令(2026-08-18「三个全做」W3~W7 = F6)
in_scope:
  - **wave.1 playtest 工具面 + 断言库**:agentd playtest.rs 编排模块;POST /api/forge/playtest/run { matrixRef } → 经 mcp::call_tool 序列执行(场景加载→play_enter→输入注入→断言求值→play_exit→结构化报告);断言库四类:entity_count / component_field / transform_near / screenshot_ssim(viewport_frame readback vs golden PNG,自实现 SSIM 确定性);断言矩阵 JSON schema( maze 用例先行);负例如实标红不充绿
  - **wave.2 回归矩阵 + 迷宫原型**:迷宫原型示例游戏(Content/Scenes/maze.rxscene + Graphs/maze_*.rxgraph + Scripts/maze.rx #[export(c)] 逻辑——玩家移动网格碰撞/门开合/终点判定,走 RD-F4-004 call_function 链);断言矩阵 tests/maze/matrix.json;test-matrix swarm 真并发(mcp.rs 非单例短连接腿:每 shard 独立 spawn engine-scene-mcp→独立 engine-host,显存预算守卫 maxConcurrent 默认 2);playtest-regression skill 工具面对齐落地;f6-w2 冒烟
  - **wave.3 Console/Metrics 面板**:Console 增强(事件类型过滤/清空/级别着色,数据=host events + playtest 报告行);Metrics tab 真数据闭环(帧统计轮询 scene_summary/viewport_frame:frames/lastTris/lastNonZeroPixels + 最近 N 采样历史表,PIE 运行中实测变化);其余占位 tab 不动
  - **wave.4 project-pack 最小打包 + engine-host --game**:打包工具(POST /api/forge/project/pack { sceneRef, outDir } → 引用闭包收集(asset_refs 链)→ Content 源 + 缓存产物 + engine-host 二进制 + 启动脚本 → 独立目录);engine-host --game <scene> 模式(加载场景→直接 play_enter→RPC 裁剪为只读+input 子集,无编辑面);f6-w4 冒烟:产物拷贝至无 workspace 干净目录启动出帧
  - **wave.5 性能 + 收官**:迷宫场景 1080p ≥60fps 实测(viewport_frame 帧计数计时窗,evidence/ 留档,对标 rurix measured 纪律);全量回归(cargo/pnpm/go/冒烟矩阵);契约 close-out
out_of_scope:
  - 资产加密/压缩打包(08 §7 逐字「不在本期」)
  - SSIM 之外的画质金标(HDR/多视角)
  - test-matrix 多机分布式(04 §5.1 逐字:单用户多 agent 并行)
  - 迷宫玩法美术打磨(灰盒原型即验收形态)
  - engine-host --game 的窗口模式(headless 出帧即验收;窗口壳后续里程碑)
deliverables:
  - id: D-F6-1
    name: playtest 工具面(agentd playtest.rs + /api/forge/playtest/run + 断言库四类 + SSIM)
    evidence: cargo test(agentd)+ 断言正负例记录
  - id: D-F6-2
    name: 迷宫原型(.rx 脚本逻辑)+ 断言矩阵 + test-matrix 真并发 + skill 对齐 + f6-w2 冒烟
    evidence: 冒烟日志 + 并发聚合报告
  - id: D-F6-3
    name: Console 增强 + Metrics 帧统计闭环
    evidence: client vitest + desktop 冒烟截图
  - id: D-F6-4
    name: project-pack + engine-host --game + f6-w4 干净目录冒烟
    evidence: 冒烟日志 + 产物清单
  - id: D-F6-5
    name: 性能实测(1080p ≥60fps)+ 全量回归
    evidence: evidence/f6-w5-perf-*.json/log + 回归输出
acceptance_gates:
  - id: G-F6-1
    name: playtest 工具门
    check: 四类断言逐一正例过 + 负例如实 FAIL(entity_count/component_field/transform_near/screenshot_ssim);fixture 场景/矩阵栈级冒烟(scripts/f6-w1-playtest-smoke.ps1:绿矩阵 ok=true + 红矩阵 failed>0 如实);SSIM 同图=1.0/异图<阈值实测;报告结构化(逐项 pass/fail + 实测值)
  - id: G-F6-2
    name: 回归矩阵门
    check: test-matrix swarm ≥2 并发 headless host(进程计数实测 ≥2 独立 engine-host)跑 maze 子矩阵聚合全绿;注入 1 个必失败断言的子矩阵如实标红(负例门);playtest-regression SKILL.md 声明工具在 MCP 面全部存在
  - id: G-F6-3
    name: Console/Metrics 门
    check: Console 过滤/清空实测可用;Metrics tab 数字来自 scene_summary/viewport_frame 实测(非占位);PIE 运行中 metrics 采样实测变化;client vitest 全绿
  - id: G-F6-4
    name: 打包门
    check: project-pack 产物含引用闭包全部资产(无缺)+ engine-host 二进制 + 启动脚本;拷贝至干净目录(脱离 workspace/无 cargo)启动 --game 出帧(viewport_frame nonZeroPixels>0);闭包外资产不入包(体积如实)
  - id: G-F6-5
    name: 性能门
    check: 迷宫场景 1080p 实测 ≥60fps(帧计数/计时窗实测均值,evidence/f6-w5-perf-*.json 留档);未达标如实 FAIL 不伪造
guardrails:
  - 诚实优先:断言失败/矩阵红/性能不达标如实登记,禁止伪造绿灯;数字必须来自命令输出
  - 并发守卫:test-matrix maxConcurrent 默认 2(显存预算,F3 推迟条款);超限排队不并爆
  - 契约 §6 只追加;.meta/契约/日志永不入密钥
  - 迷宫逻辑必须真实走 .rx call_function 链(RD-F4-004 成果用例),禁止纯图内联绕过
---

# F6 契约:试玩回归与打包

## 1. 目标与双门状态

13_ROADMAP 最终里程碑:playtest 全工具 + 断言库 + SSIM 截图断言 + test-matrix swarm 并发;playtest-regression skill 兑现;Console/Metrics 帧统计闭环;project-pack 最小打包 + engine-host --game。验收门 = 迷宫原型回归矩阵全绿 + 干净机器可运行 + 1080p ≥60fps 实测。status=active;implementation_status=unlocked。

## 2. 波次

- wave.1 → G-F6-1(playtest 工具面 + 断言库四类)。
- wave.2 → G-F6-2(迷宫原型 + 回归矩阵真并发 + skill 兑现)。
- wave.3 → G-F6-3(Console/Metrics)。
- wave.4 → G-F6-4(project-pack + --game)。
- wave.5 → G-F6-5(性能 + 全量回归 + close-out)。

## 3. 架构决策(立项裁决)

- **D-F6-A(playtest 编排落点 = agentd)**:与 D-RDG-A 同源——工具面在 agentd 进程内,playtest/run 编排 mcp::call_tool 序列(scene_load → play_enter → logic_inject_input 序列 → 断言求值[component_get/entity_list/scene_summary/viewport_frame] → play_exit → 报告)。断言库为 agentd 内纯函数模块,四类首发。SSIM 自实现(灰度 8x8 块均值方差协方差公式,确定性 f64;阈值默认 0.98 可配)。
- **D-F6-B(test-matrix 并发 = 非单例短连接腿)**:mcp.rs 现状长连接单例(场景态跨调用持久前提,不动);新增 spawn_fresh(kind) 短连接——每 shard 独立 engine-scene-mcp 子进程(→独立 engine-host 看门狗,--port 不冲突:各实例独立 TCP 监听由 engine-scene-mcp 内部自分配)。maxConcurrent 默认 2(显存预算守卫,F3 推迟条款),超出排队。分片语义:断言矩阵按 case 行 round-robin disjoint(与 swarm 既有语义一致)。
- **D-F6-C(迷宫形态)**:灰盒迷宫(墙/地板/玩家/终点/双门)实体全部 entity_create 可重建;逻辑 = maze_player.rxgraph(on_input 移动 → call.call_function maze.rx is_walkable)+ maze_door.rxgraph(on_trigger_enter → call_function 计钥匙)+ maze_goal.rxgraph(on_trigger_enter → call_function check_win);maze.rx 提供 #[export(c)] 纯值函数(is_walkable(x,z)->bool / key_count 操作 / check_win)。**必须真实走 RD-F4-004 call_function 链**(用户拍板用例),禁止图内联常量绕过。
- **D-F6-D(project-pack 形态)**:agentd REST POST /api/forge/project/pack(不落新 crate):闭包 = 场景 entity 组件 props 内资产引用 + asset_refs 递归;复制 Content 引用文件 + .meta + .forge 缓存产物(engine-host 运行所需)+ target\debug\engine-host.exe(打包期 debug 版如实标注;release 优化转后续)+ engine-host.exe 依赖(同目录 dll 实测收集)+ pack-run.ps1 启动脚本。--game 模式:engine-host 参数解析扩展(--game <sceneRel> → 启动即 scene_load + play_enter;RPC 面裁剪:禁编辑类方法,留 viewport_frame/input/logic_inject_input/host_ping/play_state 只读子集)。
- **D-F6-E(性能测量法)**:1080p(1920x1080)viewport_set_camera 就位后,viewport_frame 连续 N=300 帧计时(墙钟),fps = N/耗时;取 3 次均值;场景 = maze.rxscene play 态。evidence/f6-w5-perf-*.json 留档(均值/最小/机器配置)。未达 60fps 如实 FAIL 并登记瓶颈分析,不伪造。
- **D-F6-F(Console/Metrics 最小闭环)**:Console 增强不动事件源(host_events 轮询),加类型过滤 chips + 清空(本地视图态)+ playtest 报告注入行(role=playtest);Metrics = stats 轮询(已有 refreshSummary 链)+ 60 采样环形历史(frames/tris/nonZeroPixels 三序列,文本表+迷你条形,不引图表库)。

## 4. Deferred 处置
本波新增 deferred 追加于下方。

- **RD-F6-001(上游 rurixc --emit=dll 同模块跨函数调用符号未链接,OPEN→上游)**:wave.2 实测 maze.rx `wall_at_cell`/`solvable` 内部调用 `wall_at` 时 dll 构建失败(RX7001 clang: `use of undefined value '@rx_wall_at_8'`,同模块调用点符号名与定义符号不链接)。规避 = 导出函数全部自包含(规则内联单函数化),maze.rx 头部注释留痕;上游 H:\rurix rurixc codegen 修同模块调用链接后摘除规避。不阻塞 F6。

## 5. 修订
- 2026-08-18 立项:RD-DIGEST closed(c6fcfb7)后用户拍板 W3~W7 = F6。勘探实测:swarm.rs test-matrix shardType 已注册(F3);engine-host 仅解析 --port;Script 组件 graphRef 腿已挂接 collect_script_graphs(module 腿 seam);Console tab 已有 ConsoleBody(host events 渲染),metrics 等 5 tab 占位;MCP 面无 test_run/assert 工具;demo 资产无迷宫;08 §7 打包本期最小原文在库;SSIM 断言 F4 逐字 deferred 至 F6。

## 6. Close-out(只追加区)

### wave.1 验收记录(2026-08-18,playtest 工具面 + 断言库)→ G-F6-1 PASS
- 交付:crates/forge-agentd/src/playtest.rs(断言库四类 entity_count/component_field/transform_near/screenshot_ssim;run_matrix 编排 scene_load→camera?→play_enter→play_pause→inputs→play_step×N→cases→play_exit 兜底;unwrap_envelope 与 client 同语义;SSIM 自实现 8x8 块亮度 C1/C2 标准式);POST /api/forge/playtest/run(matrixRef;报告即诚实工件,红矩阵 ok=false 同样 200;MATRIX_NOT_FOUND 404/MATRIX_INVALID 400/PLAYTEST_TOOL_ERROR 502);tests/playtest/(fixture.rxscene + matrix_green/red/ssim_green/ssim_red);scripts/f6-w1-playtest-smoke.ps1。
- 测试数字:cargo test -p forge-agentd **53/53**(新增 playtest::tests×7——四类断言正负例/SSIM 同图=1.0 黑白<0.01/生命周期序+聚合/信封解包;路由级 404/400×2;修复 llm_chat_mock 与 gen 测试组跨锁竞态[GEN_REST_LOCK × TEST_ENV_LOCK 双持]——真实 keystore 存在后竞态显形,诚实留痕)。
- 冒烟(evidence/f6-w1-playtest-smoke-*.log 实测):绿矩阵 4/4(33ms);红矩阵 failed=3 如实(实体不存在/错位 maxDeviation=5.0);SSIM 腿——viewport_frame 实拍 960×540(nonZeroPixels=8360)→ golden PNG(R↔B 交换还原真实 RGB,System.Drawing BGRA 内存序踩坑处理)→ **同视角 ssim=1.000000 过 / 异视角(yaw 120)ssim=0.974899 < 0.999 红**,正负双例栈级实证。**G-F6-1 PASS**。

### wave.2 验收记录(2026-08-18,迷宫原型 + 回归矩阵真并发 + skill 兑现)→ G-F6-2 PASS
- 交付:迷宫原型资产(Content/Scenes/maze.rxscene[36 实体:31 墙+地板+Player+Key+Door+Goal] + Graphs/maze_{player,key,door,goal}.rxgraph + Scripts/maze.rx,全量 .meta);tests/maze/matrix{,_red}.json(28 输入碰撞敏感路线:门×2 无钥撞阻/终点后双边界撞阻,任一碰撞失效即终位失真);swarm test-matrix 真并发(main.rs 分片证据 windowMs 改绝对纪元毫秒,跨分片重叠可证);interp.rs call_function args 标量自动包一元数组(动态单参链,normalize_call_args + 单测);playtest-regression SKILL.md 摘 seam 对齐工具面;scripts/f6-w2-maze-matrix-smoke.ps1。
- 迷宫逻辑全走 .rx call_function 链(契约守卫):open_at_cell(墙)/not_door_cell(门)/is_win_cell(终点)/door_open_angle/goal_spin_angle;**fail-closed 语义**(call_error → 分支 Null → 阻断移动,dll 失效 = 玩家冻结 = 矩阵必红,不会穿透放行)。
- 踩坑留痕:①**RD-F6-001**——rurixc --emit=dll 同模块跨函数调用符号未链接(RX7001 @rx_wall_at_8 undefined),规避 = 导出函数自包含内联;②初版路线自相抵消(相对输入回程反向等长,碰撞失效不可见)→ 重写为门撞阻+终点后不可抵消边界撞阻;③初版 fail-open(call_error → else → 穿透)→ 语义反转为 fail-closed;④IDE 脏缓冲坑×2(main.rs/冒烟脚本编辑不落盘,终端 WriteAllText 修复)。
- 测试数字:cargo test --workspace 全绿(211:agentd 53 / forge-logic 41[含 call_args_scalar_autowrap 新测] / 其余 crate 117)。
- 冒烟(evidence/f6-w2-maze-matrix-smoke-*.log):绿全矩阵 **6/6**(实体计数/玩家终点/钥匙下沉/门开下沉/终点升起/winner 标签,270ms);swarm 绿 2 分片聚合 **6/0**,pid 互异(52776/63188),时间窗重叠(True),**engine-host 进程峰值=3**(单例+2 FreshSession ≥2 实测);swarm 红子矩阵 errors=2 如实标红(错位 maxDeviation=4.0/门未开 3.0);SKILL 声明 12 引擎工具在 mcp/tools(75)全部存在。**G-F6-2 PASS**。

### wave.3 验收记录(2026-08-18,Console/Metrics 面板)→ G-F6-3 PASS
- 交付:client consoleUtils.ts(consoleLevel/filterEvents/typeCounts/ringPush 纯函数);editorStore metricsHistory 三序列采样环(refreshSummary 追加 cap 60)+ runPlaytest(apiPost /api/forge/playtest/run → 报告行注入 events,role=playtest);EditorView ConsoleBody(类型过滤 chips 带计数/清空本地视图态/级别着色 error 红 + playtest 蓝)+ MetricsBody(1s 轮询 + 当前值 + 60 采样迷你条形 + 近 8 值文本表);host forgeProxy + '/api/forge/playtest' 前缀;desktop main.cjs console-metrics 冒烟场景;scripts/f6-w3-console-metrics-smoke.ps1。
- 测试数字:client vitest **82/82**(新增 consoleMetrics.test.ts×7——级别/过滤/清空下标/计数/环 cap×2/报告注入红绿);host vitest 19/19;`pnpm -r typecheck` 全绿。
- 冒烟(evidence/f6-w3-console-metrics-smoke-*.log + desktop-smoke-console-metrics-*.png 106KB):metrics tab **frames 采样 PIE 运行中实测变化**(recent: 1 10 13 16 19 22,play_enter 驱动);Console playtest.report 注入行=1;类型过滤 chip 点击后 playtest.case 行=0;清空后报告行=0。踩坑:metrics 腿 play_enter 后单例 host 滞留 play_running,矩阵 scene_load 被拒 → 场景内补 play_exit(留痕)。**G-F6-3 PASS**。

### wave.4 验收记录(2026-08-18,project-pack + engine-host --game)→ G-F6-4 PASS
- 交付:crates/forge-agentd/src/pack.rs(collect_closure BFS:scene → graphRef 等 Content/ 引用 → .rxgraph → call_function module 链;**先读后收**——不可读引用 warning 如实且不入闭包;仅 .rxscene/.rxgraph/.json 递归扫,.rx 等文本为叶子;.meta 存在才收;collect_rxdll 收集 {stem}-*.dll 缺则 warning;build_pack 校验 PACK_SCENE_NOT_FOUND/PACK_ENGINE_MISSING/PACK_OUTDIR_CONFLICT/PACK_SCENE_OUTSIDE_CONTENT);POST /api/forge/project/pack(项目根 = 场景向上首个含 Content/ 祖先;engine_bin = target\debug\engine-host.exe 如实 debugEngine=true);engine-host --game(main.rs parse_game[--game/--game=/FORGE_GAME_SCENE 兜底] + 绑定后 game_boot = scene_load→play_enter→game_mode=true→FORGE_HOST_GAME_BOOTED;rpc.rs GAME_ALLOWED 14 方法只读+input 子集,编辑面一律 -32601「game 模式禁编辑面」;game_mode_rejects_edit_surface 单测 9 拒 5 放);scripts/f6-w4-pack-smoke.ps1。
- 踩坑留痕:①main() 内 --game 调用块两次 IDE 脏缓冲未落盘(parse_game 函数在但从未被调,干净目录只 LISTENING 不 GAME_BOOTED)→ 终端 WriteAllText 修复 + Select-String 核验;②collect_closure 初版 insert 先于 read(缺失引用误入闭包)+ .rx 非 JSON 误警告 → 修为先读后收 + jsonish 叶子判定(pack 单测×3:bfs 链/缺失警告/布局排除);③PS foreach 语句缺括号解析错;④PS 5.1 2>&1+Stop stderr 误抛 → 构建段切 Continue 按 exit code 判定(复用既有坑对策)。
- 测试数字:cargo test engine-host **24/24** + forge-agentd **56/56**(pack::tests×3 新增)。
- 冒烟(evidence/f6-w4-pack-smoke-*.log 实测):pack 报告**闭包 11 项无缺**(maze 场景+4 图+全量 .meta+maze.rx);闭包外 8 抽样(Main/call_probe/door_opener/f4w3_probe/callprobe.rx/wood_albedo/w4_mat/f5w2_chair)未入包;产物含 bin/engine-host.exe + pack-run.ps1 + .forge/cache/rxdll/maze-6838b13c60161d39.dll;**totalBytes=12,837,054** warnings=0。**拷贝至干净目录($env:TEMP,脱离 workspace)经 pack-run.ps1 启动**:LISTENING port=17890 + GAME_BOOTED;scene.summary **36 实体 + play_running**;viewport.frame 960×540 **nonZeroPixels=72,517** draws=36(RTX 4070 Ti)出帧实证;entity.create **-32601 拒**(编辑面裁剪)。**G-F6-4 PASS**。
