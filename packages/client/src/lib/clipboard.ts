import { useToastStore } from './toastStore';

/**
 * 复制文本到剪贴板并给出 toast 回执(右键菜单的「复制…」项共用)。
 * 剪贴板 API 在非安全上下文 / jsdom 下不存在,如实报失败——菜单项已经点下去了,
 * 静默无回执会让人以为复制成功。
 */
export async function copyText(text: string, what: string): Promise<boolean> {
  const { push } = useToastStore.getState();
  try {
    const write = globalThis.navigator?.clipboard?.writeText;
    if (typeof write !== 'function') throw new Error('clipboard unavailable');
    await globalThis.navigator.clipboard.writeText(text);
    push('success', `已复制${what}`);
    return true;
  } catch {
    push('error', `复制${what}失败`);
    return false;
  }
}
