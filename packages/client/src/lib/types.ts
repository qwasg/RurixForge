/** 全局共享类型(F7 wave.3:会话侧 mock 类型随旧面下线;以下保留给 components/agent 渲染层)。 */

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
