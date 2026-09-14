import { create } from 'zustand';
import { useWorkbenchStore } from './workbenchStore';

/**
 * D-035:Plan 模式的前端状态面。
 *
 * 计划本体不在这里——它是工作区文件,由 PlanTab 经 workspace/file API 读写(与文件编辑器
 * 同一套保存/冲突纪律)。本 store 只管三件与会话相关的事:
 * - activePlanPath:当前会话的计划文件(快照 activeSession.activePlanPath + plan.* 事件);
 * - planning:当前是否有 plan 模式的 run 在跑(PlanTab 头部显示「正在调研…」);
 * - reloadNonce:后端覆盖了计划文件(plan.updated)→ 通知已开着的 PlanTab 重载。
 */

interface PlanState {
  activePlanPath: string | null;
  /** plan 模式 run 进行中(agent 正在调研 / 改计划)。 */
  planning: boolean;
  /** 计划文件被后端改写的计数;PlanTab 以它为 effect 依赖重载。 */
  reloadNonce: number;

  setActivePlanPath: (path: string | null) => void;
  setPlanning: (v: boolean) => void;
  /** plan.created / plan.updated 到达:回填路径、打开页签、触发重载。 */
  onPlanEvent: (path: string, created: boolean) => void;
  reset: () => void;
}

export const usePlanStore = create<PlanState>((set, get) => ({
  activePlanPath: null,
  planning: false,
  reloadNonce: 0,

  setActivePlanPath: (path) => {
    if (get().activePlanPath !== path) set({ activePlanPath: path });
  },

  setPlanning: (v) => {
    if (get().planning !== v) set({ planning: v });
  },

  onPlanEvent: (path, created) => {
    set((st) => ({ activePlanPath: path, reloadNonce: st.reloadNonce + 1 }));
    // 新计划落盘即开页签(Cursor 语义);迭代覆盖时若页签已关也重新开,
    // 用户刚要求改计划,结果不该只落在磁盘上无声无息。
    void created;
    useWorkbenchStore.getState().openPlan(path);
  },

  reset: () => set({ activePlanPath: null, planning: false, reloadNonce: 0 }),
}));

/**
 * Build 时随消息发出的一句话正文(真正的任务书由后端读计划文件注入,D-034:不拼正文前缀)。
 * 抽成函数只为让测试与 PlanTab 共用同一文案。
 */
export function buildPrompt(planName: string): string {
  return `按计划《${planName}》实施`;
}
