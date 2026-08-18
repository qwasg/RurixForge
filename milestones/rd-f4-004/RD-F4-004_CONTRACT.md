---
contract: RD-F4-004
title: RD-F4-004 回填波(call.call_function 互绑)
status: closed
implementation_status: unlocked
active_scope: wave.1
version: 0.1
date: 2026-08-18
timebox: 会话制推进,做不完转 deferred
rfc_required: []
upstream_docs:
  - 10_INTERACTION_LOGIC.md (§4.2 节点注册表 / §4.3 与 .rx 互绑)
  - 05_MCP_PROJECTS.md (§4 code-forge 注记)
  - 13_ROADMAP.md (F4 交付 2:call_function 互绑 .rx)
  - milestones/f4/F4_CONTRACT.md (D-F4-H / RD-F4-004 refill 定义)
implementation_unlock:
  required_all:
    - F5 验收门全绿(wave.1~4 §6 已录,status=closed,commit f0653ee)
    - 用户开工指令(2026-08-18「三个全做」排序拍板 W1→W2→F6)
in_scope:
  - 导出表扫描器(forge-logic rxexport.rs):文本级解析 .rx 源 `#[export(c)] pub fn` 条目 → {name, params[(name, ty)], ret};如实标注文本级非语义级(照 code_symbol_search 先例,D-RD4-A)
  - graph_validate 第九校验臂(call_function):module 文件存在(项目相对,10 §4.3 形态 Content/Scripts/*.rx)+ fn 在导出表 + args 对 C 兼容子集 v1 签名匹配;三新错误码 GRAPH_CALL_MODULE_NOT_FOUND / GRAPH_CALL_FN_NOT_EXPORTED / GRAPH_CALL_SIG_MISMATCH
  - 节点 schema 加性扩展:call.call_function registry 增 result 输出 pin(Any;10 §4.3「纯值进/出」兑现;冻结 40 节点集不变,D-RD4-D)
  - 运行时 dll 腿(forge-logic callruntime.rs):rurixc 直包 --emit=dll(rx CLI RX7003 不透传 dll,D-F4-A 先例)→ .forge/cache/rxdll/ 缓存(键=源 SHA-256+rurixc 版本串)→ libloading 加载 → 标量编组调用(D-RD4-B/C)
  - interp.rs call.call_function 摘 logic.unsupported:读 module/fn/args → invoke → 返回值供 result pin 数据求值;构建/加载/fn 缺失/超子集如实 logic.call_error 续链(不静默不伪造)
  - scripts/rd-f4-004-call-function-smoke.ps1 栈级冒烟
out_of_scope:
  - 数组/指针/String/Vec3 编组(C 子集 v1 含指针,图 PinType 交集外;首发标量 f32/f64/i32/bool + void/标量返回)
  - 调用超时/死循环防护(同步调用;subset v1 无 panic 面 RXS-0255 编译期保证,死循环风险如实标注)
  - rx 脚本 → 图 send_message 反向链(F4 已落地 event.on_message,本波复核闭环不重建)
  - hot-reload dll(module 文件变更重构建;照 D-F4-B 热重载语义,play 态 component_set Script → 实例重建时缓存键自然失效)
deferred_refs: [RD-F4-001, RD-F4-002, RD-F4-003]
deliverables:
  - id: D-RD4-1
    name: 导出表扫描器 + graph_validate 第九校验臂(三新错误码)+ result pin
    evidence: cargo test(forge-logic)+ 校验臂三码逐类拒绝记录
  - id: D-RD4-2
    name: callruntime dll 腿 + interp 摘 unsupported + 栈级冒烟
    evidence: cargo test + scripts/rd-f4-004-call-function-smoke.ps1 输出
acceptance_gates:
  - id: G-RD4-1
    name: 校验臂门
    check: fixture .rx(#[export(c)] pub fn add/is_ready 等)在库 → 好图(module+fn+args 匹配)graph_validate 零错误;三类坏图(module 不存在 / fn 未导出 / args 数与类型不匹配或超标量子集)逐类返 GRAPH_CALL_MODULE_NOT_FOUND / GRAPH_CALL_FN_NOT_EXPORTED / GRAPH_CALL_SIG_MISMATCH;扫描器文本级如实标注进工具描述/注释
  - id: G-RD4-2
    name: 运行时门
    check: 栈级冒烟(经 gateway→agentd→code-forge-mcp + engine-scene-mcp):图 event.on_start → call.call_function {module: add.rx, fn: add, args: [2, 3]} → var.set result → play_enter → component/黑板断言 result=5;.forge/cache/rxdll/ 产物落盘,复跑缓存命中零重建;fn 缺失图 play 态如实 logic.call_error(host_events_drain 可见)不静默
guardrails:
  - 诚实优先:任何门不过如实报 FAIL;dll 构建/加载失败不伪造返回值
  - 数字必须来自命令输出;契约 §6 只追加
  - 密钥红线 R-5 不涉(本波无密钥面)
  - 双状态机:status/implementation_status 严格分离
---

# RD-F4-004 契约:call.call_function 互绑回填

## 1. 目标与双门状态

兑现 F4 交付 2 未竟项(13_ROADMAP F4「`call_function` 互绑 `.rx`」)与 RD-F4-004 refill 定义:「graph_validate 加 call_function 校验臂(module 存在 + #[export(c)] fn 表 + 签名匹配)+ dll 运行时加载」。status=active;implementation_status=unlocked。

## 2. 范围与波次

- wave.1:导出表扫描器 + graph_validate 第九校验臂(三新错误码)+ result pin 扩展 → G-RD4-1。
- wave.2:callruntime dll 腿(rurixc --emit=dll + 缓存 + libloading 编组)+ interp 摘 unsupported → G-RD4-2。
- wave.3:栈级冒烟 + 全量回归 + close-out。

## 3. 架构决策(立项裁决,全部实测锚定)

- **D-RD4-A(fn 表来源 = 文本扫描器)**:RD-F4-004 refill 原设「#[export(c)] fn 表经 rurixc --emit=reflection」。**实测复核(2026-08-18):--emit=reflection 对 #[export(c)] pub fn 宿主函数 entries=[](rurix.shader-reflection.v1 schema,shader-only,RXS-0304)**——F4 wave.4 勘探结论成立,fn 表不能走 reflection。落地:forge-logic 文本扫描器解析 `#[export(c)] pub fn` 条目头(照 code_symbol_search 文本级先例,如实标注「文本级非语义级」);编译期真子集校验由 dll 构建腿 rurixc 把关(RX6031/6033 结构化诊断透传)。
- **D-RD4-B(dll 通道 = rurixc 直包)**:**实测 rx build --emit=dll 不存在(RX7003:合法 check/mir/llvm-ir/nvptx-ir/ptx/pyd)**;rurixc 驱动层支持 --emit=dll(driver.rs 合法集 check/mir/reflection/permutations/capabilities/rt-manifest/llvm-ir/nvptx-ir/ptx/pyd/dll;实测 `rurixc probe.rx --emit=dll` exit=0,产 .dll + .h C 原型 RXS-0253 + .lib/.exp)。照 D-F4-A 先例(rx_check 直包 rurixc)直包 rurixc;FORGE_RURIXC env 复用,缺省 H:\rurix\target\debug\rurixc.exe。缓存键 = 源文件 SHA-256 + rurixc --version 串,产物落 .forge/cache/rxdll/(照 assetd rxmesh 缓存先例)。
- **D-RD4-C(编组子集 = 标量首发)**:上游 C 兼容子集 v1 = i8~u64/f32/f64/bool + Ptr + void(export_c.rs RXS-0251);图 PinType 交集 = F32/I32/Bool。首发:f32/f64/i32/bool 参数 + void/f32/f64/i32/bool 返回;String/Vec3/数组/指针不编组(校验臂 SIG_MISMATCH 如实拒,不静默截断)。libloading 0.8 进 forge-logic(workspace 本无此依赖,2026-08-18 grep 实测;windows LoadLibrary 腿,跨平台由 libloading 抽象)。
- **D-RD4-D(result pin 加性扩展)**:10 §4.3「纯值进/出」要求返回值可用;现 registry outputs=[]。增 `result: Any` 输出 pin——冻结子集为 40 节点集(不变),pin 扩展属加性(照 D-F4-E/F 注册表扩展先例,登记决策)。
- **D-RD4-E(运行时错误语义)**:dll 构建失败/加载失败/fn 缺失/arg 类型运行时不匹配 → logic.call_error 事件(带 module/fn/原因)进 host 事件环,续执行链;不静默吞不伪造返回(照 logic.unsupported 先例)。同步调用无超时——subset v1 无 panic 面(RXS-0255 编译期结构性保证),死循环风险如实标注(play 线程同生命周期)。
- **D-RD4-F(测试隔离)**:dll 构建走真实 rurixc(H:\rurix 在库,F4 先例);cargo 测试 fixture 落 tests/fixtures/rd-f4-004/*.rx;并发构建同 module 加文件锁(照 SUBAGENTS_DIR_LOCK 先例)。

## 4. Deferred 处置
本波新增 deferred 追加于下方。

(暂无)

## 5. 修订
- 2026-08-18 立项:F5 全绿(commit f0653ee)后用户指令「三个全做」拍板 W1→W2→F6 顺序,本波为首波。勘探实测纠偏两处方:①rx CLI 无 --emit=dll(RX7003),dll 通道在 rurixc 直包;②--emit=reflection shader-only(entries=[]),fn 表走文本扫描器。上游 export_c.rs C 子集 v1/RXS-0250~0255 就绪;.rx 脚本 → 图 send_message → event.on_message 反向链 F4 已落地,本波不重做。

## 6. Close-out(只追加区)

### wave.1 验收记录(2026-08-18)

- 验收门:G-RD4-1(校验臂门)
- 结果:PASS
- 证据:
  - cargo test -p forge-logic 40/40 全绿:rxexport 扫描器 5(基本/name 覆写/跨行签名/非导出忽略/非 pub 拒)+ 校验臂 5(好图过/MODULE_NOT_FOUND 含越界同码/FN_NOT_EXPORTED/SIG_MISMATCH 四类(个数/类型/超子集/非数组)/纯 validate_graph 八臂语义不动)
  - 三新错误码落地:GRAPH_CALL_MODULE_NOT_FOUND / GRAPH_CALL_FN_NOT_EXPORTED / GRAPH_CALL_SIG_MISMATCH;同构性门(D-RD4-C 运行时无混合类型蹦床,校验期如实拒)
  - 注册表加性扩展:call.call_function + result:Any 输出 pin(40 节点集不变,registry 计数测试不动)
  - 调用方切换:graphtool(graph_validate/graph_create)+ engine-host 装图(load_graph_doc)皆走 validate_graph_with_project
- 交付:crates/forge-logic/src/rxexport.rs(文本级扫描器,如实标注「文本级非语义级;编译期真校验由 rurixc --emit=dll 把关」)
- 踩坑:
  1. **IDE 脏缓冲坑本波三连发**:graphtool.rs 四处编辑两处不落盘 / interp.rs 结构体字段三处不落盘 / 冒烟脚本头两处不落盘——PS [IO.File]::WriteAllText 无 BOM 补丁 + Select-String 核验为固定对策(每次编辑后必核验)。
  2. E0277 &String==String 比较一处。

### wave.2 验收记录(2026-08-18)

- 验收门:G-RD4-2(运行时门)
- 结果:PASS
- 证据:
  - cargo test:callruntime 2 测试全过——**真实 dll 调用链**:fixture math.rx(add/mul3/negate/not/forty_two/poke)→ rurixc --emit=dll 真建 → libloading 加载 → **add(2.0,3.5)=5.5 / mul3(2,3,4)=24 / negate(7)=-7 / not(true)=false / forty_two()=42 / poke→void Null**;错误面五类(FnNotExported/Sig 个数/Sig 运行时类型/ModuleNotFound/越界同码)
  - cargo test --workspace **191/191 全绿**(F5 基线 179 + 新增 12)
- 交付:crates/forge-logic/src/callruntime.rs(CallRuntime:缓存键=源 SHA-256+rurixc.exe SHA-256 前 16hex,.forge/cache/rxdll/;并发文件锁;CallError 五类)+ interp.rs(call.call_function 摘 unsupported:node_outputs 动作节点输出暂存 + eval_pin 命中 + logic.call/logic.call_error 事件)+ LogicRuntime.call_rt + set_project_root + engine-host play_enter 接线
- 实测锚定纠偏(子智能体谎报两处,亲测推翻):
  1. ~~rx build --emit=dll~~ **不存在**(RX7003:合法 check/mir/llvm-ir/nvptx-ir/ptx/pyd);dll 通道在 rurixc 直包(driver.rs 合法集含 dll;实测 probe.rx exit=0 产 .dll+.h+.lib)。
  2. ~~libloading 已在 workspace~~ ** grep 实测无**,本波新增 forge-logic 依赖。
  3. rurixc 产物落点:无 -o 时落**源文件旁**;-o 对 --emit=dll 生效(全体副产物同落 -o 目录)——构建腿直出缓存键名。
  4. --emit=reflection 对 #[export(c)] pub fn 产 entries=[](shader-only,RXS-0304 复核),fn 表走文本扫描器。
- 踩坑:
  1. ensure_dll 漏定义 abs(笔误)+ 产物落点误判(以为 CWD,实为源旁)→ -o 实测后直出缓存键名。
  2. 宏两坑:$fty:ty 不可拼贴于 extern "C" 后(改完整类型传入)/ args[0] 非单 token(:tt → :expr)。
  3. cargo test -p forge-logic 不重建 code-forge-mcp 二进制——栈级冒烟前须 cargo build --workspace(冒烟首跑拿旧 exe 误判校验臂失效)。

### wave.3 验收记录(2026-08-18):栈级冒烟 + 全量回归 + close-out 终审

- 证据:
  - scripts/rd-f4-004-call-function-smoke.ps1 **PASS(连跑两遍幂等)**:经 gateway→agentd→code-forge-mcp/engine-scene-mcp——①校验臂三码:好图 ok / mixed(f32,i32) 混合签名 GRAPH_CALL_SIG_MISMATCH / ghost.rx GRAPH_CALL_MODULE_NOT_FOUND;②运行时:scene_new → entity_create Script(call_probe.rxgraph)→ play_enter → drain **logic.call fn=add result=5.5**(2+3.5,真实 dll 调用);③缓存:callprobe-0ff3fb379249a82b.dll 落盘,play_exit 重进 result=5.5 再现 + **LastWriteTimeUtc 不变零重建** + 产物唯一(缓存键不漂移);④错误腿:NodePin 供 bool 作 args → **logic.call_error reason 含 args 语义**(不静默不伪造,服务端 UTF-8;PS 5.1 显示面 ISO-8859-1 乱码系已知显示坑)
  - 全量回归:cargo test --workspace **191/191**(0 failed);pnpm -r typecheck/test **97/97**(protocol 5 + client 73 + host 19)/ build 全绿;go build+test exit=0;零孤儿进程
- 交付:scripts/rd-f4-004-call-function-smoke.ps1;fixture 留档:projects/demo/Content/Scripts/callprobe.rx(add + mixed 两导出)+ Content/Graphs/call_probe.rxgraph(+.meta)
- 踩坑:
  1. **PS 5.1 函数返回一元逗号 `return ,$arr` 坑**:输出为单一项(数组整体),调用方 @() 不再摊平 → ConvertTo-Json 呈 {value:[...],Count} 幻形,排查三轮方定位;正解 = 枚举返回 `return $arr`(隔离实验实证)。
  2. 冒烟漏 JWT(401)/Select-Object -First 1 与数组成员枚举两形态——断言一律 @(...)[0].prop 显式取首。
  3. 补丁草稿残块(scriptblock `= {` 误植 hashtable)——PS 补丁后必读回核验。
- 终审:**G-RD4-1 校验臂门 PASS + G-RD4-2 运行时门 PASS,RD-F4-004 回填完成。call_function 互绑双向链闭合(图→.rx dll 调用 / .rx→图 send_message→on_message F4 已落地)。status: closed → closed。**
- 登记 deferred:无新增(混合参数类型/数组/指针/Vec3 编组随真实需求再立;死循环防护维持 D-RD4-E 如实标注)。
