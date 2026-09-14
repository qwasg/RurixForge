/**
 * 当前工作区 id 的无依赖读取面。
 *
 * workspaceStore 是事实源(并把当前值镜像到 localStorage);forgeApi 每次 MCP 调用都要带上
 * workspaceId 给 agentd 做项目作用域解析,但 workspaceStore 又依赖 forgeApi 的 apiGet/apiPost,
 * 直接互相 import 会成环。这里只读 localStorage 镜像键,两侧都依赖本模块而互不依赖。
 */

export const ACTIVE_WORKSPACE_KEY = 'forge:activeWorkspace';

/** 当前工作区 id;无/未选 → null。 */
export function readActiveWorkspaceId(): string | null {
  try {
    const v = localStorage.getItem(ACTIVE_WORKSPACE_KEY);
    return v === null || v === '' || v === 'null' ? null : v;
  } catch {
    return null;
  }
}

/** 写镜像键(null = 清除)。 */
export function writeActiveWorkspaceId(id: string | null): void {
  try {
    if (id === null) localStorage.removeItem(ACTIVE_WORKSPACE_KEY);
    else localStorage.setItem(ACTIVE_WORKSPACE_KEY, id);
  } catch {
    /* ignore */
  }
}
