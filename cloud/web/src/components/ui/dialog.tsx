import * as DialogPrimitive from '@radix-ui/react-dialog';
import { X } from 'lucide-react';
import type { ComponentPropsWithoutRef, ReactNode } from 'react';
import { cn } from '@/lib/utils';
import { Button } from './button';

export const Dialog = DialogPrimitive.Root;
export const DialogClose = DialogPrimitive.Close;

const SIZES = {
  sm: 'max-w-md',
  md: 'max-w-xl',
  lg: 'max-w-3xl',
  xl: 'max-w-5xl',
} as const;

type ContentProps = Omit<ComponentPropsWithoutRef<typeof DialogPrimitive.Content>, 'title'> & {
  title: ReactNode;
  description?: ReactNode;
  footer?: ReactNode;
};

function Header({ title, description }: { title: ReactNode; description?: ReactNode }) {
  return (
    <div className="flex shrink-0 items-start justify-between gap-4 border-b px-5 py-3">
      <div className="min-w-0">
        <DialogPrimitive.Title className="text-base font-semibold leading-6">{title}</DialogPrimitive.Title>
        {description ? (
          <DialogPrimitive.Description className="mt-0.5 text-xs text-muted-foreground">{description}</DialogPrimitive.Description>
        ) : null}
      </div>
      <DialogPrimitive.Close asChild>
        <Button variant="ghost" size="icon-sm" aria-label="关闭" className="-mr-2">
          <X />
        </Button>
      </DialogPrimitive.Close>
    </div>
  );
}

/** 居中对话框：内容放在可滚动的遮罩里，主体区域自身滚动，页脚固定。 */
export function DialogContent({
  title,
  description,
  footer,
  size = 'md',
  className,
  children,
  ...props
}: ContentProps & { size?: keyof typeof SIZES }) {
  return (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Overlay className="fixed inset-0 z-50 flex animate-fade-in items-start justify-center overflow-y-auto bg-black/40 p-4 pt-[6vh]">
        <DialogPrimitive.Content
          {...(description ? {} : { 'aria-describedby': undefined })}
          className={cn(
            'relative flex max-h-[calc(94vh-2rem)] w-full animate-dialog-in flex-col rounded-lg border bg-card shadow-pop focus:outline-none',
            SIZES[size],
            className,
          )}
          {...props}
        >
          <Header title={title} description={description} />
          <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">{children}</div>
          {footer ? <div className="flex shrink-0 items-center justify-end gap-2 border-t px-5 py-3">{footer}</div> : null}
        </DialogPrimitive.Content>
      </DialogPrimitive.Overlay>
    </DialogPrimitive.Portal>
  );
}

/** 右侧抽屉（详情面板）。 */
export function DrawerContent({
  title,
  description,
  footer,
  className,
  children,
  ...props
}: ContentProps) {
  return (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Overlay className="fixed inset-0 z-50 animate-fade-in bg-black/30" />
      <DialogPrimitive.Content
        {...(description ? {} : { 'aria-describedby': undefined })}
        className={cn(
          'fixed inset-y-0 right-0 z-50 flex w-full max-w-3xl animate-slide-in-right flex-col border-l bg-card shadow-pop focus:outline-none',
          className,
        )}
        {...props}
      >
        <Header title={title} description={description} />
        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">{children}</div>
        {footer ? <div className="flex shrink-0 items-center justify-end gap-2 border-t px-5 py-3">{footer}</div> : null}
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  );
}
