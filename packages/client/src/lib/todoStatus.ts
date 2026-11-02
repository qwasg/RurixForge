/** 交接验证与外部阻塞不能冒充制作完成或普通实现失败。 */
export type TodoStatus = 'pending' | 'running' | 'verifying' | 'completed' | 'failed' | 'blocked';

export function normalizeTodoStatus(status: string): TodoStatus {
  switch (status) {
    case 'running':
    case 'in_progress':
      return 'running';
    case 'verifying':
      return 'verifying';
    case 'blocked':
      return 'blocked';
    case 'completed':
    case 'done':
      return 'completed';
    case 'failed':
    case 'review':
      return 'failed';
    default:
      return 'pending';
  }
}
