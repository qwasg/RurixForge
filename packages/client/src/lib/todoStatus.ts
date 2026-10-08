/** 计划文件与执行事件使用不同状态名称，统一为待办圆圈的四种显示状态。 */
export type TodoStatus = 'pending' | 'running' | 'completed' | 'failed';

export function normalizeTodoStatus(status: string): TodoStatus {
  switch (status) {
    case 'running':
    case 'in_progress':
      return 'running';
    case 'completed':
    case 'done':
      return 'completed';
    case 'failed':
    case 'blocked':
    case 'review':
      return 'failed';
    default:
      return 'pending';
  }
}
