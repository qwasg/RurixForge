/** 全局共享类型:侧栏数据、会话视图数据、右侧面板。 */

export interface SidebarAgent {
  id: string;
  title: string;
  ago: string;
  /** 蓝色圆点(进行中的会话) */
  active?: boolean;
  /** 云会话标记(小云图标) */
  cloud?: boolean;
  /** 分支会话标记(ago 前显示分支图标) */
  branch?: boolean;
  /** 悬浮详情卡:仓库 / 分支 / 本地路径 */
  repo?: string;
  branchName?: string;
  path?: string;
}

export interface Workspace {
  id: string;
  name: string;
  agents: SidebarAgent[];
  /** 底部显示 "More" 行 */
  hasMore?: boolean;
}

export interface AutomationTemplate {
  id: string;
  category: string;
  title: string;
  description: string;
  trigger: string;
  action: string;
}

export interface CustomizeItem {
  id: string;
  name: string;
  description: string;
}

/** 会话视图中的内容块(按时间顺序渲染)。 */
export type ConversationBlock =
  | { kind: 'user'; text: string }
  | { kind: 'thought'; label: string }
  | { kind: 'markdown'; md: string }
  | { kind: 'muted-line'; label: string }
  | { kind: 'subagents'; items: SubagentRun[] }
  | { kind: 'waiting'; label: string };

export interface SubagentRun {
  id: string;
  title: string;
  badge: string;
  /** 运行期间依次轮转的状态行 */
  statusSequence: string[];
  /** 完成后的最终状态行 */
  doneLabel: string;
  done: boolean;
  /** 展开卡片里的 prompt 与活动日志 */
  prompt: string;
  activity: string[];
  /** 完成后的报告 markdown */
  report?: string;
}

export interface FileNode {
  name: string;
  type: 'file' | 'dir';
  children?: FileNode[];
}
