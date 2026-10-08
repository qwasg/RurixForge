import * as Menu from '@radix-ui/react-dropdown-menu';
import type { ComponentPropsWithoutRef } from 'react';
import { cn } from '@/lib/utils';

export const DropdownMenu = Menu.Root;
export const DropdownMenuTrigger = Menu.Trigger;

export function DropdownMenuContent({
  className,
  align = 'end',
  sideOffset = 4,
  ...props
}: ComponentPropsWithoutRef<typeof Menu.Content>) {
  return (
    <Menu.Portal>
      <Menu.Content
        align={align}
        sideOffset={sideOffset}
        className={cn('z-50 min-w-40 animate-fade-in rounded-md border bg-card p-1 text-sm shadow-pop', className)}
        {...props}
      />
    </Menu.Portal>
  );
}

export function DropdownMenuItem({
  className,
  danger = false,
  ...props
}: ComponentPropsWithoutRef<typeof Menu.Item> & { danger?: boolean }) {
  return (
    <Menu.Item
      className={cn(
        'flex cursor-default select-none items-center gap-2 rounded px-2 py-1.5 outline-none data-[disabled]:pointer-events-none data-[highlighted]:bg-accent data-[disabled]:opacity-40 [&_svg]:size-4 [&_svg]:shrink-0',
        danger ? 'text-danger' : 'text-foreground',
        className,
      )}
      {...props}
    />
  );
}

export function DropdownMenuSeparator({ className, ...props }: ComponentPropsWithoutRef<typeof Menu.Separator>) {
  return <Menu.Separator className={cn('-mx-1 my-1 h-px bg-border', className)} {...props} />;
}
