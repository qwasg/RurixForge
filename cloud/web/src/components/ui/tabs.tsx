import * as TabsPrimitive from '@radix-ui/react-tabs';
import type { ComponentPropsWithoutRef } from 'react';
import { cn } from '@/lib/utils';

export const Tabs = TabsPrimitive.Root;

export function TabsList({ className, ...props }: ComponentPropsWithoutRef<typeof TabsPrimitive.List>) {
  return <TabsPrimitive.List className={cn('flex gap-1 overflow-x-auto border-b', className)} {...props} />;
}

export function TabsTrigger({ className, ...props }: ComponentPropsWithoutRef<typeof TabsPrimitive.Trigger>) {
  return (
    <TabsPrimitive.Trigger
      className={cn(
        '-mb-px inline-flex items-center gap-1.5 whitespace-nowrap border-b-2 border-transparent px-3 py-2 text-sm text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40 data-[state=active]:border-foreground data-[state=active]:font-medium data-[state=active]:text-foreground [&_svg]:size-4',
        className,
      )}
      {...props}
    />
  );
}

/** 不要给它加 display 类：forceMount 时靠 hidden 属性隐藏非活动面板。 */
export function TabsContent({ className, ...props }: ComponentPropsWithoutRef<typeof TabsPrimitive.Content>) {
  return <TabsPrimitive.Content className={cn('pt-4 focus-visible:outline-none', className)} {...props} />;
}
