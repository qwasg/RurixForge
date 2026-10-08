import { useCallback, useEffect, useRef, useState, type DependencyList, type Dispatch, type SetStateAction } from 'react';

export interface AsyncState<T> {
  data: T | undefined;
  error: unknown;
  loading: boolean;
  reload: () => void;
  setData: Dispatch<SetStateAction<T | undefined>>;
}

/**
 * 拉数据：deps 变化或 reload() 时重新请求；过期响应丢弃。
 * 重新加载期间保留旧 data，表格不闪空。
 */
export function useAsync<T>(fn: () => Promise<T>, deps: DependencyList): AsyncState<T> {
  const [data, setData] = useState<T>();
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [tick, setTick] = useState(0);
  const fnRef = useRef(fn);
  fnRef.current = fn;

  useEffect(() => {
    let current = true;
    setLoading(true);
    fnRef.current().then(
      (value) => {
        if (!current) return;
        setData(value);
        setError(null);
        setLoading(false);
      },
      (err: unknown) => {
        if (!current) return;
        setError(err);
        setLoading(false);
      },
    );
    return () => {
      current = false;
    };
    // fn 通过 ref 读取最新值，依赖由调用方显式给出
  }, [...deps, tick]);

  const reload = useCallback(() => setTick((t) => t + 1), []);
  return { data, error, loading, reload, setData };
}

export function useDebounced<T>(value: T, delayMs = 300): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setDebounced(value), delayMs);
    return () => clearTimeout(t);
  }, [value, delayMs]);
  return debounced;
}

/** 每 intervalMs 刷新一次的当前时间（倒计时用）。 */
export function useNow(intervalMs = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(t);
  }, [intervalMs]);
  return now;
}

export interface PaginationState {
  limit: number;
  offset: number;
  setOffset: (offset: number) => void;
  setLimit: (limit: number) => void;
  reset: () => void;
}

export function usePagination(initialLimit = 20): PaginationState {
  const [limit, setLimitState] = useState(initialLimit);
  const [offset, setOffset] = useState(0);
  const setLimit = useCallback((l: number) => {
    setLimitState(l);
    setOffset(0);
  }, []);
  const reset = useCallback(() => setOffset(0), []);
  return { limit, offset, setOffset, setLimit, reset };
}
