import '@testing-library/jest-dom/vitest';

// jsdom 无 ResizeObserver(ViewportCanvas 帧协商用):最小桩,仅使组件可挂载;
// 行为断言(帧请求/点选/gizmo)均走真实 mock 后端,不由本桩代绿。
class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
if (typeof globalThis.ResizeObserver === 'undefined') {
  (globalThis as Record<string, unknown>).ResizeObserver = ResizeObserverStub;
}

// F9:jsdom 的 Range 无布局面(getClientRects/getBoundingClientRect)——
// CodeMirror 异步测量阶段(requestAnimationFrame)会在文本 Range 上调用它们,
// 缺桩时抛 unhandled error 使 vitest 非零退出(不体现为红测试)。空几何即可,
// CM 自身兼容无布局环境;高亮/编辑行为断言仍走真实 DOM,不由本桩代绿。
function emptyDomRect(): DOMRect {
  const r = { x: 0, y: 0, top: 0, left: 0, bottom: 0, right: 0, width: 0, height: 0 };
  return { ...r, toJSON: () => r } as DOMRect;
}
class DomRectListStub extends Array<DOMRect> {
  item(i: number): DOMRect | null {
    return this[i] ?? null;
  }
}
if (typeof globalThis.Range !== 'undefined') {
  const proto = globalThis.Range.prototype as Range & {
    getBoundingClientRect?: () => DOMRect;
    getClientRects?: () => DOMRectList;
  };
  proto.getBoundingClientRect ??= emptyDomRect;
  proto.getClientRects ??= () => new DomRectListStub() as unknown as DOMRectList;
}
