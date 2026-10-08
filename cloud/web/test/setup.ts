import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

// vitest 未开 globals，testing-library 不会自动注册 cleanup。
afterEach(() => {
  cleanup();
});

// jsdom 没有 ResizeObserver（Radix 的部分组件会用到）。
if (typeof globalThis.ResizeObserver === 'undefined') {
  class ResizeObserverStub {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  }
  (globalThis as Record<string, unknown>).ResizeObserver = ResizeObserverStub;
}
