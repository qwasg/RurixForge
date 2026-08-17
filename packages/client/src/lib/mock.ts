import type {
  AutomationTemplate,
  ConversationBlock,
  CustomizeItem,
  FileNode,
  SidebarAgent,
  Workspace,
} from './types';
import type { ForgeAPI } from './bridge';

/* ---------------- 侧栏 ---------------- */

export const PINNED_AGENTS: SidebarAgent[] = [
  {
    id: 'pin-1',
    title: 'AI-driven game engine design',
    ago: '13d',
    repo: 'qwasg/rurix',
    branchName: 'main',
    path: 'H:\\rurix',
  },
  {
    id: 'pin-2',
    title: 'Image-io interface specification',
    ago: '1mo',
    branch: true,
    repo: 'qwasg/rurix',
    branchName: 'main',
    path: 'H:\\rurix',
  },
];

/** Workspaces "+" 弹层里的最近文件夹。 */
export const WORKSPACE_RECENTS = [
  'D:\\游戏引擎',
  'D:\\agent-cowork',
  'D:\\cindy',
  'D:\\非遗',
  'I:\\非遗',
  'I:\\agent-debug-frontend-backend-copy-20260530',
];

export const WORKSPACES: Workspace[] = [
  {
    id: 'ws-game',
    name: '游戏引擎',
    agents: [
      { id: 'a-open-ide', title: '打开前端 IDE', ago: '11m' },
      { id: 'a-rust-fb', title: 'Rust frontend and backend', ago: '20m' },
    ],
  },
  {
    id: 'ws-cowork',
    name: 'agent-cowork',
    agents: [{ id: 'a-fork', title: 'GitHub project fork', ago: '17h' }],
  },
  {
    id: 'ws-cindy',
    name: 'cindy',
    agents: [{ id: 'a-cindy', title: 'Forking Cindy repository', ago: '17h', active: true }],
  },
  {
    id: 'ws-feiyi',
    name: '非遗',
    hasMore: true,
    agents: [
      { id: 'a-gesture', title: 'Gesture operation mapping', ago: '1d' },
      { id: 'a-clean', title: '清理 Cursor 项目痕迹', ago: '1d' },
    ],
  },
  {
    id: 'ws-rurix',
    name: 'rurix',
    hasMore: true,
    agents: [
      { id: 'a-g61', title: 'G6.1功能介绍', ago: '1d' },
      { id: 'a-g61c', title: 'G6.1 completion and gaps', ago: '1d', active: true },
      { id: 'a-g61p', title: 'G6.1 project discussion', ago: '1d' },
      { id: 'a-upstream', title: 'Upstream issue status', ago: '1d' },
      { id: 'a-remove', title: 'Remove GitHub agent collaborators', ago: '1d' },
    ],
  },
];

/* ---------------- 搜索面板 ---------------- */

export interface PaletteRow {
  id: string;
  label: string;
  group: 'Agents' | 'Files' | 'Actions' | 'Settings';
  workspace?: string;
  ago?: string;
  shortcut?: string;
}

export const PALETTE_ROWS: PaletteRow[] = [
  { id: 'p1', label: 'Remove GitHub agent collaborators', group: 'Agents', workspace: 'rurix', ago: '1d' },
  { id: 'p2', label: 'Frontend and backend preview', group: 'Agents', workspace: '游戏引擎', ago: '12m' },
  { id: 'p3', label: 'Gesture operation mapping', group: 'Agents', workspace: '非遗', ago: '1d' },
  { id: 'p4', label: '协议天女子Agent集群', group: 'Agents', workspace: '小说', ago: '1d' },
  { id: 'p5', label: 'AI character design and novel', group: 'Agents', workspace: '小说', ago: '1d' },
  { id: 'p6', label: 'GitHub project fork', group: 'Agents', workspace: 'agent-cowork', ago: '17h' },
  { id: 'p7', label: '清理 Cursor 项目痕迹', group: 'Agents', workspace: '非遗', ago: '1d' },
  { id: 'p8', label: 'web-design-engineer', group: 'Agents', workspace: 'rurix', ago: '1d' },
  { id: 'p9', label: 'Frontend and backend preview', group: 'Files', workspace: '非遗', ago: '1d' },
  { id: 'p10', label: 'Light Theme', group: 'Settings' },
  { id: 'p11', label: 'Dark Theme', group: 'Settings' },
  { id: 'p12', label: 'System Theme', group: 'Settings' },
  { id: 'p13', label: 'High Contrast Theme', group: 'Settings' },
  { id: 'p14', label: 'Reload Window', group: 'Actions', shortcut: 'Ctrl Shift R' },
  { id: 'p15', label: 'Toggle Developer Tools', group: 'Actions', shortcut: 'Ctrl Shift I' },
  { id: 'p16', label: 'Go Back', group: 'Actions', shortcut: 'Alt ←' },
  { id: 'p17', label: 'Go Forward', group: 'Actions', shortcut: 'Alt →' },
  { id: 'p18', label: 'Open Conversation Logs Folder', group: 'Actions' },
  { id: 'p19', label: 'Split Tile Horizontally', group: 'Actions' },
  { id: 'p20', label: 'Split Tile Vertically', group: 'Actions' },
  { id: 'p21', label: 'Pin / Unpin Agent', group: 'Actions' },
  { id: 'p22', label: 'Search Agents', group: 'Actions', shortcut: 'Ctrl Alt P' },
  { id: 'p23', label: 'Plan Mode', group: 'Actions' },
  { id: 'p24', label: 'Agent Mode', group: 'Actions' },
];

/* ---------------- Automations 页 ---------------- */

export const AUTOMATION_CATEGORIES = [
  'Popular',
  'Code Review',
  'Security',
  'Incidents & Triage',
  'Data & Research',
  'Environment',
];

export const AUTOMATION_TEMPLATES: AutomationTemplate[] = [
  {
    id: 't1',
    category: 'Popular',
    title: 'Find critical bugs',
    description: 'Analyze recent commits for high-severity correctness bugs',
    trigger: 'Scheduled',
    action: 'Send Slack',
  },
  {
    id: 't2',
    category: 'Security',
    title: 'Scan codebase for vulnerabilities',
    description: 'Review the full repository on a schedule and alert on validated high-impact security issues',
    trigger: 'Scheduled',
    action: 'Send Slack',
  },
  {
    id: 't3',
    category: 'Popular',
    title: 'Add test coverage',
    description: 'Review recent changes and add tests for high-risk logic that lacks adequate coverage',
    trigger: 'Scheduled',
    action: 'Send Slack',
  },
  {
    id: 't4',
    category: 'Incidents & Triage',
    title: 'Investigate anomalies in Slack setup failures',
    description: 'Analyze recent commits for high-severity correctness bugs and submit safe fixes before merge',
    trigger: 'Scheduled',
    action: 'Send Slack',
  },
  {
    id: 't5',
    category: 'Incidents & Triage',
    title: 'Monitor environment builds health',
    description: 'Health-check four cloud environment builds and auto-approve low-risk PRs for failed builds',
    trigger: 'Scheduled',
    action: 'Send Slack',
  },
  {
    id: 't6',
    category: 'Code Review',
    title: 'Fix CI failures',
    description: 'Detect CI failures on main and automatically file PRs that checks completed',
    trigger: 'Scheduled',
    action: 'Send Slack',
  },
  {
    id: 't7',
    category: 'Code Review',
    title: 'Auto-fix PR review comments',
    description: 'Triage new issues by investigating bugs, planning feature requests, and opening PRs for easy fixes',
    trigger: 'PR review comment',
    action: 'PR Comment',
  },
  {
    id: 't8',
    category: 'Data & Research',
    title: 'Investigate top Datadog errors',
    description: 'Investigate recurring production errors from Datadog. Re-check root causes only when a rule regresses',
    trigger: 'Scheduled',
    action: 'Send Slack',
  },
  {
    id: 't9',
    category: 'Popular',
    title: 'Investigate Sentry issues',
    description: 'Investigate errors from Sentry, identify root causes, and analyze recent commits for high-severity correctness bugs',
    trigger: 'Any issue event',
    action: 'Send Slack',
  },
  {
    id: 't10',
    category: 'Popular',
    title: 'Triage Linear issues',
    description: 'Triage new issues by investigating bugs, planning feature requests, and opening PRs for easy fixes',
    trigger: 'Issue created',
    action: 'Send Slack',
  },
];

/* ---------------- Customize 页 ---------------- */

export const CUSTOMIZE_TABS = [
  'Plugins',
  'MCPs',
  'Skills',
  'Subagents',
  'Rules',
  'Commands',
  'Hooks',
] as const;

export type CustomizeTab = (typeof CUSTOMIZE_TABS)[number];

export const CUSTOMIZE_ITEMS: Record<CustomizeTab, CustomizeItem[]> = {
  Plugins: [
    { id: 'pl1', name: 'always-respond-in-Chinese-simplified', description: 'User context invocation reading, creating or referring external data sources like Linear, Figma, and Notion.' },
  ],
  MCPs: [
    { id: 'm1', name: 'playwright', description: 'Use when the task requires automating a real browser from the terminal (navigation, form filling, snapshots, screenshots).' },
    { id: 'm2', name: 'review-agent', description: 'Perform a read-only, defect-first review of a specified code change and return every actionable finding. Use when another agent finishes code.' },
    { id: 'm3', name: 'ui-ux-pro-max', description: 'UI/UX design intelligence with searchable database.' },
    { id: 'm4', name: 'web-design-engineer', description: 'Build high-quality visual Web artifacts using HTML/CSS/JavaScript/React — web pages, landing pages, dashboards, internal tools.' },
  ],
  Skills: [
    { id: 's1', name: 'commit', description: 'Create a well-formed git commit from the current change set.' },
    { id: 's2', name: 'review', description: 'Agentic code review of your changes.' },
  ],
  Subagents: [
    { id: 'sa1', name: 'Explorer', description: 'Fast codebase exploration with prompt-enforced read-only behavior.' },
    { id: 'sa2', name: 'review-agent', description: 'Perform a read-only, defect-first review of a specified code change.' },
  ],
  Rules: [
    { id: 'r1', name: 'always-chinese', description: 'Always respond in Chinese-simplified.' },
  ],
  Commands: [
    { id: 'c1', name: '/review', description: 'Agentic code review of your changes.' },
    { id: 'c2', name: '/babysit', description: 'Triage PR comments, fix CI failures, and clear conflicts.' },
  ],
  Hooks: [
    { id: 'h1', name: 'Automate with Hooks', description: 'Hooks run custom scripts at lifecycle events to observe, control, and extend the agent loop.' },
  ],
};

/* ---------------- Agent 会话(视频 2) ---------------- */

export const AGENT_TITLE = 'Parallel agent exploration project';
export const AGENT_BRANCH = 'cursor/g5-rd038-w1w2-degradation-waves';
export const AGENT_MODEL = 'Cursor Grok 4.5 High Fast';

export const CONVERSATION: ConversationBlock[] = [
  { kind: 'user', text: '帮我调用并行智能体探索项目并给出建议' },
  { kind: 'thought', label: 'Thought briefly' },
  {
    kind: 'markdown',
    md: '我会并行启动多个探索智能体，从不同角度摸清项目结构与现状，再汇总建议。',
  },
  { kind: 'muted-line', label: 'Explored available tools' },
  {
    kind: 'subagents',
    items: [
      {
        id: 'sub-1',
        title: '探索项目整体结构',
        badge: 'Explorer',
        statusSequence: ['探索项目结构与配置', '分析入口点与架构', '整理探索报告'],
        doneLabel: '完成 Rurix 代码库全面探索',
        done: true,
        prompt:
          'Explore the codebase at H:/rurix thoroughly (very thorough). Return a structured summary in Chinese (简体中文) covering:\n1. Project type/purpose (what does this project do?)\n2. Top-level directory structure and role of each major folder\n3. Tech stack (languages, frameworks, build tools, package managers)\n4. Entry points (how the app/system boots)\n5. Key configuration files (package.json, Cargo.toml, pyproject, docker, etc)\n6. Monorepo vs single package? Workspace setup?\n7. Documentation quality (README, docs/)\n8. Any obvious architectural patterns\n\nBe concrete: cite file paths. Do not modify anything. Focus on orientation for someone who needs improvement suggestions.',
        activity: [
          'Thought briefly',
          '正在全面探索 H:/rurix 代码库，先从顶层结构与关键配置文件入手。',
          'Exploring 4 searches, ran 1 command',
          'Updated progress',
          'Searching files * in rurix',
          'Search files attempted',
          'Ran ls "h:\\rurix"',
          'Searching files * in rurix',
        ],
      },
      {
        id: 'sub-2',
        title: '探索核心业务代码',
        badge: 'Explorer',
        statusSequence: ['探索项目结构与模块', '分析核心 crate 结构', '分析测试与依赖关系', '整理分析报告'],
        doneLabel: '完成代码库核心业务逻辑分析',
        done: true,
        prompt:
          'Explore the core application/business logic of the codebase at H:/rurix (very thorough). Return a structured summary in Chinese (简体中文) covering:',
        activity: [
          'Thought briefly',
          '正在全面探索代码库的核心业务逻辑，先从项目结构与主要模块入手。',
          'Explored 27 files, 24 searches, ran 4 commands',
          'Worked for 1m 1s',
        ],
        report:
          '**Rurix 代码库核心业务逻辑分析报告**\n\n**1. 主要模块 / 领域**\n\n| 模块 | 路径 | 职责 |\n|---|---|---|\n| 编译器 | `H:/rurix/src/rurixc` | 前端 (lexer/parser/AST) → HIR → 类型检查 → 借用/资源检查 → MIR → 三后端 codegen (NVPTX/DXIL/SPIR-V) → host 链接 |\n| 运行时 | `H:/rurix/src/rurix-rt` | CUDA Driver API 薄层;affine 资源生命周期编译期拦截;多后端 (Direct / RXS 区间) |',
      },
      {
        id: 'sub-3',
        title: '探索工程与依赖质量',
        badge: 'Explorer',
        statusSequence: ['探索项目工程化配置', '整理工程化评估报告'],
        doneLabel: '完成工程化质量评估',
        done: true,
        prompt:
          'Explore engineering quality of the project at H:/rurix (medium thoroughness). Return a structured summary in Chinese (简体中文) covering:',
        activity: [
          'Thought briefly',
          '正在探索项目工程化配置，先从根目录结构与关键配置文件入手。',
          'Explored 22 files, 33 searches, ran 6 commands',
          'Updated progress',
          'Greped name = "(rowan|rapier3d|nanobind|serde|tokio|regex)"',
          'Read rust-toolchain.toml L1-5',
          'Read CI_GATEs.md L1-50',
          'Ran List workspace root contents',
        ],
      },
    ],
  },
  { kind: 'waiting', label: 'Waiting for subagent' },
  { kind: 'thought', label: 'Thought briefly' },
  {
    kind: 'markdown',
    md: '三个探索智能体已并行完成分析(⌄ 整体结构、⌄ 核心业务、⌄ 工程化)。结论如下。\n\n**项目画像**\n\nRurix 是 Rust 实现的 GPU 系统编程语言 + 完整工具链(编译器 `rurixc`、运行时 `rurix-rt`、CLI `rx`、三后端 NVPTX/DXIL/SPIR-V)。治理与 conformance 体系成熟(278/278 规范锚定、~350 用例、三层 CI),语言侧已到 v1.0 / G5 收口,G6 物理轨在推进。仓库里还混有相对独立的 `genesis/`、`external/godot` 等,容易干扰导航。\n\n**优先建议**\n\n**P0 — 收益最大**\n\n1. **拆分超大模块**:`vk.rs`(~15k 行)、`render_exec.rs`、`typeck.rs`、`mir_build.rs` 是主要维护瓶颈。按后端/阶段拆 submodule,能立刻降低 review 与回归成本。\n2. **CI 拆出 CPU 快路径**:PR Smoke 绑在单一 Windows GPU 自托管机上、90+ 串行步骤。把 fmt/clippy/guardrails/traceability/无 GPU test 拆到 cloud runner,GPU「快路径」单独成队。\n3. **依赖治理自动化**:加 cargo-deny + cargo-audit(及 Dependabot)。主栈依赖少,补齐成本低、风险覆盖高。\n\n**P2 — 中期**\n\n7. 补齐 CLI:`rx fix` / `rx watch`(已有诊断数据源可接)。\n8. Device 验收阈值收拢到配置 `/evidence/`,减少 bin 内硬编码 TODO。\n9. Python CI 脚本加 ruff;`requirements.txt` 精确 pin。\n10. 长期跟踪图形双轨简化(RD-015),减轻 SPIR-V→HLSL 外链依赖。\n\n**不必急着动的**\n\n- 规范 ↔ conformance ↔ PR 三角治理——这是核心竞争力,应保留,只优化「入口摩擦」而不是砍门禁。\n- Workspace `unsafe_code = deny` + `unsafe-audit/`——已经很强。\n- `measured_local` 性能预算——继续用,别为了速度放松。\n\n若要落地,建议下一刀从「CI 快路径拆分」或「`vk.rs` 模块拆分」二选一开工;需要的话我可以直接动手做其中一项。',
  },
];

/* ---------------- 右侧 Files 面板 ---------------- */

export const FILE_TREE: FileNode[] = [
  {
    name: 'rurix',
    type: 'dir',
    children: [
      { name: '.cargo', type: 'dir' },
      { name: '.claude', type: 'dir' },
      { name: '.cursor', type: 'dir' },
      { name: '.github', type: 'dir' },
      { name: '.kiro', type: 'dir' },
      { name: '.pytest_cache', type: 'dir' },
      { name: '.tmp', type: 'dir' },
      { name: '.trae', type: 'dir' },
      { name: '.vscode', type: 'dir' },
      { name: '.workbuddy', type: 'dir' },
      { name: '.writing', type: 'dir' },
      { name: '渲染器调研', type: 'dir' },
      { name: 'agents', type: 'dir' },
      { name: 'apps', type: 'dir' },
      { name: 'bench', type: 'dir' },
      { name: 'build', type: 'dir' },
      { name: 'channels', type: 'dir' },
      { name: 'ci', type: 'dir' },
      { name: 'conformance', type: 'dir' },
      { name: 'deep-research', type: 'dir' },
      { name: 'evidence', type: 'dir' },
      { name: 'external', type: 'dir' },
      { name: 'genesis', type: 'dir' },
      { name: 'guide', type: 'dir' },
      { name: 'milestones', type: 'dir' },
      { name: 'NVIDIA Corporation', type: 'dir' },
      { name: 'promo', type: 'dir' },
      { name: 'registry', type: 'dir' },
      { name: 'rfcs', type: 'dir' },
      { name: 'showcase', type: 'dir' },
      { name: 'spec', type: 'dir' },
      { name: 'spike', type: 'dir' },
      { name: 'src', type: 'dir' },
      { name: 'target', type: 'dir' },
      { name: 'tests', type: 'dir' },
      { name: 'unsafe-audit', type: 'dir' },
      { name: '.gitattributes', type: 'file' },
      { name: '.gitignore', type: 'file' },
      { name: '00_MASTER_INDEX.md', type: 'file' },
      { name: '01_VISION_AND_MISSION.md', type: 'file' },
      { name: '02_USERS_AND_USE_CASES.md', type: 'file' },
      { name: '03_POSITIONING_AND_LANDSCAPE.md', type: 'file' },
      { name: '04_DESIGN_PRINCIPLES.md', type: 'file' },
      { name: '05_LANGUAGE_ARCHITECTURE.md', type: 'file' },
      { name: 'Cargo.lock', type: 'file' },
      { name: 'Cargo.toml', type: 'file' },
      { name: 'CODE_OF_CONDUCT.en.md', type: 'file' },
      { name: 'CODE_OF_CONDUCT.md', type: 'file' },
      { name: 'CONTRIBUTING.en.md', type: 'file' },
      { name: 'CONTRIBUTING.md', type: 'file' },
      { name: 'LICENSE-APACHE', type: 'file' },
      { name: 'LICENSE-MIT', type: 'file' },
      { name: 'README.md', type: 'file' },
      { name: 'requirements.txt', type: 'file' },
      { name: 'rurix.lock', type: 'file' },
      { name: 'rust-toolchain.toml', type: 'file' },
      { name: 'SECURITY.md', type: 'file' },
    ],
  },
];

/* ---------------- Bridge 降级(纯 web 环境) ---------------- */

/**
 * 纯 web 环境下的 window.forgeAPI 桩:窗口控制全部 no-op,
 * onMaximizedChanged 返回 no-op 取消订阅函数,platform 标记为 'web'。
 * 供 lib/bridge.ts 在 window.forgeAPI 不存在时回退使用。
 */
export const MOCK_FORGE_API: ForgeAPI = {
  win: {
    minimize: () => {},
    toggleMaximize: () => {},
    close: () => {},
    onMaximizedChanged: () => () => {},
  },
  platform: 'web',
};
