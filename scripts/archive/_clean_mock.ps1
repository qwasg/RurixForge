$ErrorActionPreference = 'Stop'
$p = "packages\client\src\lib\mock.ts"
$lines = [System.Collections.Generic.List[string]][IO.File]::ReadAllLines($p)
# 行区间(1-based 包含)手术:清空演示数据,保留类型/MOCK_FORGE_API。
$out = New-Object System.Collections.Generic.List[string]
function Keep($a, $b) { for ($i = $a; $i -le $b; $i++) { $out.Add($lines[$i - 1]) } }
function Put([string[]]$rows) { foreach ($r in $rows) { $out.Add($r) } }

Keep 1 12
Put @('export const PINNED_AGENTS: SidebarAgent[] = []; // 演示数据已清空(如实空态,不伪造)')
Keep 32 33
Put @('export const WORKSPACE_RECENTS: string[] = [];')
Keep 42 42
Put @('export const WORKSPACES: Workspace[] = [];')
Keep 84 95
Put @('export const PALETTE_ROWS: PaletteRow[] = [];')
Keep 122 124
Put @('export const AUTOMATION_CATEGORIES: string[] = [];')
Keep 133 133
Put @('export const AUTOMATION_TEMPLATES: AutomationTemplate[] = [];')
Keep 216 218
Put @('export const CUSTOMIZE_TABS = [] as const;')
Keep 228 230
Put @('export const CUSTOMIZE_ITEMS: Record<CustomizeTab, CustomizeItem[]> = {};')
Keep 260 262
Put @("export const AGENT_TITLE = '';", "export const AGENT_BRANCH = '';", "export const AGENT_MODEL = 'deepseek-chat'; // 与 agentd 真实 provider 对齐")
Keep 266 266
Put @('export const CONVERSATION: ConversationBlock[] = [];')
Keep 345 347
Put @('export const FILE_TREE: FileNode[] = [];')
Keep 413 429

[IO.File]::WriteAllText($p, ($out -join "`n") + "`n")
"WRITTEN $($out.Count) lines"
