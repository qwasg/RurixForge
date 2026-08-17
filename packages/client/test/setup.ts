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
