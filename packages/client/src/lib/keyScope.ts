/**
 * 画布快捷键的作用域判定(画板与素材创作两处画布共用)。
 * 快捷键挂在可聚焦的画布容器上,键盘事件会从卡上的输入框冒泡上来——
 * 焦点在输入类元素里时必须把键让回去,否则打字会触发「删除节点」之类的动作。
 */
export function isTyping(t: EventTarget | null): boolean {
  const el = t as HTMLElement | null;
  if (el === null || typeof el.tagName !== 'string') return false;
  return (
    el.tagName === 'INPUT' ||
    el.tagName === 'TEXTAREA' ||
    el.tagName === 'SELECT' ||
    el.isContentEditable === true
  );
}
