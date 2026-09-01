# Tasks

- [x] Task 1: 环境编排与基线核验——确认 client dist 最新（必要时 `pnpm --filter @forge/client build`）；拉起/复用 agentd(8103) 与 host(3080)（查端口与进程命令行+启动时间，本仓 dev 实例才可重启，留痕）；内置浏览器打开 http://127.0.0.1:3080，基线截图（首页零白屏、三栏渲染、SSE 连接建立）
- [x] Task 2: 壳交互巡检——侧栏（新建会话/搜索/置顶/双击重命名/文件夹/More 展开）、TitleBar 四菜单+搜索胶囊（浏览器态窗口三钮确认隐藏）、Ctrl+K 命令面板（执行 1 条命令）、StatusBar 段核验；逐面截图 evidence/f9-journey-shell-*
- [x] Task 3: 设置五页巡检——外观（主题明暗切换实测 --accent 变化+截图）/Agent/模型（deepseek 与 openai-compat 渠道卡 availability 实测）/技能（列表渲染）/关于（版本与 health）；逐页截图 evidence/f9-journey-settings-*
- [x] Task 4: workbench + 底部面板 + Inspector 巡检——tabs（plan/todo/diff/editor 切换）、底部面板（Agent Logs/Output/Metrics 开合拖拽）、Inspector 树展开+文件点击真实预览（F8 wave.1 成果）；禁用态核验（资产右键「导入/在文件夹显示」诚实禁用）；逐面截图 evidence/f9-journey-workbench-*
- [x] Task 5: 全项目制作 journey·场景与实体——新建会话（命名「journey 项目制作」）；经聊天 agent 创建场景实体（地形/玩家/门/终点，指令措辞参照 f8-w4 T4/T5 已验证面）；编辑器 tab 打开视口出帧截图（nonZeroPixels>0）；Hierarchy 实体清单截图；entity_list 后端核验
- [x] Task 6: 全项目制作 journey·材质与逻辑——为实体赋予材质/贴图（经 Assets 面板既有资产或 gen local-mock 生成，禁用态面走聊天 agent 路径）；挂载触发开门逻辑图（复用 door_opener 模板或 agent 生成）；NodeGraph 面板截图；asset_refs 后端核验
- [x] Task 7: 全项目制作 journey·保存/试玩/打包——保存场景（scene 文件落盘核验）；PIE play_enter 试玩截图；playtest 断言（经 /api/forge/playtest/run 或聊天 agent，报告核验）；project-pack 打包（产物目录+引用闭包清单核验）；每步截图 evidence/f9-journey-build-*
- [x] Task 7.5（checklist 复验衍生）: 缺陷修复——D1 Hierarchy 同步（editorStore entityCount 漂移重拉+EditorView 1s 轮询）；D5 host forgeProxy +/api/forge/project 前缀；D6 pack 闭包 guid 索引解析+二进制可读性修正；三修复经浏览器 live 复验 PASS
- [x] Task 8: 汇总与留档——汇总 JSON（每面/每步独立 verdict+截图路径+pageErrors/consoleErrors+失败根因）落 evidence/f9-journey-summary-<UTC>.json；失败项如实登记不充绿；零孤儿进程核查；本 spec checklist 逐项核验打勾

# Task Dependencies

- [Task 2] [Task 3] [Task 4] depends on [Task 1]（同一内置浏览器实例，串行执行；顺序可互换）
- [Task 5] depends on [Task 1]（journey 与巡检可交错，但同一浏览器串行）
- [Task 6] depends on [Task 5]
- [Task 7] depends on [Task 6]
- [Task 8] depends on [Task 2] [Task 3] [Task 4] [Task 7]
