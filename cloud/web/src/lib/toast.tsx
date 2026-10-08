import { CircleCheck, CircleX, Info, X } from 'lucide-react';
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { errorMessage } from './api/client';
import { cn } from './utils';

type ToastKind = 'success' | 'error' | 'info';

interface ToastItem {
  id: number;
  kind: ToastKind;
  message: string;
}

export interface ToastApi {
  success(message: string): void;
  info(message: string): void;
  /** 接受 ApiError / Error / 字符串，显示 error.message。 */
  error(err: unknown): void;
  dismiss(id: number): void;
}

const noop = () => undefined;
const ToastContext = createContext<ToastApi>({ success: noop, info: noop, error: noop, dismiss: noop });

const DURATION: Record<ToastKind, number> = { success: 3000, info: 4000, error: 6000 };
const MAX_TOASTS = 5;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const nextId = useRef(1);
  const timers = useRef(new Map<number, ReturnType<typeof setTimeout>>());

  const dismiss = useCallback((id: number) => {
    const t = timers.current.get(id);
    if (t) clearTimeout(t);
    timers.current.delete(id);
    setItems((prev) => prev.filter((i) => i.id !== id));
  }, []);

  const push = useCallback(
    (kind: ToastKind, message: string) => {
      const id = nextId.current++;
      setItems((prev) => [...prev, { id, kind, message }].slice(-MAX_TOASTS));
      timers.current.set(
        id,
        setTimeout(() => dismiss(id), DURATION[kind]),
      );
    },
    [dismiss],
  );

  useEffect(() => {
    const map = timers.current;
    return () => {
      for (const t of map.values()) clearTimeout(t);
      map.clear();
    };
  }, []);

  const api = useMemo<ToastApi>(
    () => ({
      success: (m) => push('success', m),
      info: (m) => push('info', m),
      error: (err) => push('error', errorMessage(err)),
      dismiss,
    }),
    [push, dismiss],
  );

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div aria-live="polite" className="fixed bottom-4 right-4 z-[60] flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-2">
        {items.map((t) => (
          <div
            key={t.id}
            role={t.kind === 'error' ? 'alert' : 'status'}
            className={cn(
              'flex animate-toast-in items-start gap-2 rounded-md border bg-card px-3 py-2.5 text-sm shadow-pop',
              t.kind === 'error' && 'border-danger/40',
            )}
          >
            {t.kind === 'success' ? (
              <CircleCheck className="mt-px size-4 shrink-0 text-success" />
            ) : t.kind === 'error' ? (
              <CircleX className="mt-px size-4 shrink-0 text-danger" />
            ) : (
              <Info className="mt-px size-4 shrink-0 text-info" />
            )}
            <p className="min-w-0 flex-1 break-words">{t.message}</p>
            <button
              type="button"
              aria-label="关闭提示"
              className="shrink-0 rounded text-muted-foreground hover:text-foreground"
              onClick={() => dismiss(t.id)}
            >
              <X className="size-3.5" />
            </button>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastApi {
  return useContext(ToastContext);
}
