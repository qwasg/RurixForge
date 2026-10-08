import { useCallback, useRef, useState, type ReactNode } from 'react';
import { useToast } from '@/lib/toast';
import { Button } from './button';
import { Dialog, DialogContent } from './dialog';

export interface ConfirmOptions {
  title: ReactNode;
  description?: ReactNode;
  confirmText?: string;
  danger?: boolean;
  /** 失败时弹错误提示并保持对话框打开。 */
  action: () => Promise<unknown> | unknown;
}

/** 返回 [对话框元素, 打开函数]；一个页面放一个即可复用于多处删除/作废。 */
export function useConfirm(): [ReactNode, (opts: ConfirmOptions) => void] {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const optsRef = useRef<ConfirmOptions | null>(null);
  const toast = useToast();

  const ask = useCallback((opts: ConfirmOptions) => {
    optsRef.current = opts;
    setOpen(true);
  }, []);

  const run = async () => {
    const opts = optsRef.current;
    if (!opts) return;
    setBusy(true);
    try {
      await opts.action();
      setOpen(false);
    } catch (err) {
      toast.error(err);
    } finally {
      setBusy(false);
    }
  };

  const opts = optsRef.current;
  const element = (
    <Dialog open={open} onOpenChange={(o) => !busy && setOpen(o)}>
      {opts ? (
        <DialogContent
          size="sm"
          title={opts.title}
          footer={
            <>
              <Button disabled={busy} onClick={() => setOpen(false)}>
                取消
              </Button>
              <Button variant={opts.danger ? 'danger' : 'primary'} loading={busy} onClick={run}>
                {opts.confirmText ?? '确定'}
              </Button>
            </>
          }
        >
          <div className="text-sm leading-6 text-muted-foreground">{opts.description ?? '此操作会立即生效，确定继续吗？'}</div>
        </DialogContent>
      ) : null}
    </Dialog>
  );
  return [element, ask];
}
