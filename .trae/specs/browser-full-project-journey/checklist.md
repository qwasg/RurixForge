# Checklist

- [x] client dist 为最新构建，agentd(8103)/host(3080) 运行正常，基线截图证明首页零白屏、三栏渲染、SSE 已连接（f9-journey-baseline-1.png）
- [x] 侧栏交互全部真实响应：新建会话/搜索/置顶/重命名/文件夹/More，截图落档（f9-journey-shell-1~5）
- [x] TitleBar 菜单与搜索胶囊可用；浏览器态窗口三钮确认隐藏（DOM 取证 winCtrls=0；f9-journey-shell-6~9）
- [x] Ctrl+K 命令面板打开并成功执行至少 1 条命令（14 命令，执行「打开设置」；f9-journey-shell-10）
- [x] 设置五页逐页渲染：外观主题切换 --accent computed 值实测变化并截图（#C96442↔#E2886A）；模型页 deepseek+openai-compat 渠道卡 availability 为真实后端值（available/needs-key；f9-journey-settings-1~6）
- [x] workbench 四 tab 切换正常；底部面板 Agent Logs/Output/Metrics 开合与高度拖拽正常（四 tab+三子面板截图落档；拖拽把手存在，浏览器工具不可驱动如实降级登记，拖拽 clamp 逻辑由 client vitest bottomPanel 10 用例机器覆盖）
- [x] Inspector 树懒加载展开；文件点击呈现真实只读预览（00_MASTER_INDEX.md 7348B，非 toast 占位）
- [x] 浏览器禁用态核验：资产「导入/在文件夹显示」为诚实禁用态，无崩溃无伪造（disabled+title「仅桌面端可用」）
- [x] journey 会话创建成功；聊天 agent 真实创建场景实体（entity_list 36→37，JourneyBox#37 cube@[2,1,2]，deepseek live 工具循环）
- [x] 编辑器视口浏览器出帧且 nonZeroPixels>0（canvas readback 腿，159825→354529 递增），Hierarchy 清单与实体一致，截图落档（D1 缺陷修复后 live 复验：MCP 创建 ReverifyProbe 后 3s 面板 36→37 自动同步；修复后构建截图因工具链超时未产出，快照日志辅证 browser-logs/snapshot-2026-08-19T13-12-25-218Z.log）
- [x] 材质/贴图真实绑定实体（w4_mat guid 入 MeshRenderer.material，asset_refs 核验）；逻辑图挂载成功（door_opener.rxgraph，NodeGraph 4 节点截图）
- [x] 场景保存落盘（journey.rxscene 20,471B 真实存在且含 JourneyBox+w4_mat guid+door_opener 引用）
- [x] PIE play_enter 成功且视口运行态截图落档（edit→play_running→edit，物理 steps 8213→11641）
- [x] playtest 断言报告生成且结果如实记录（tests/maze/matrix.json ok:true passed:6 failed:0；journey 专属矩阵因 D4 降级复用，如实登记）
- [x] project-pack 产物目录真实存在，含引用闭包资产+engine-host 二进制+启动脚本（D5/D6 修复后复验：经 3080 单源 200 出包 17 项含 w4_mat.rxmat+f2w4_dot.png+engine-host.exe 12,153,856B+pack-run.ps1）
- [x] 汇总 JSON 含全部独立 verdict/截图路径/pageErrors=0（或如实登记）/失败根因，落仓 evidence/f9-journey-summary-2026-08-19T11-30-00Z.json
- [x] 全程未修改任何产品代码（巡检与 journey 阶段）；checklist 复验衍生的 D1/D5/D6 三缺陷修复按 spec 第七步流程落地并经 live 复验；结束后零孤儿进程（主线拉起的 agentd/host 在收尾时停止并核查端口/进程）
