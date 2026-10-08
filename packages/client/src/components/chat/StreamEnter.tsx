import { useEffect, useRef, useState, type ReactNode } from 'react';
import { cn } from '@/lib/cn';

/**
 * D-047 流式新块的入场包装(.forge-stream-in:自上而下展开 + 轻微下落淡入,见 theme.css)。
 *
 * 是否播入场在挂载那一刻定下,之后不随 props 变:class 中途加上会让早已在屏上的块重播,
 * 流式结束时摘掉又会让播到一半的块跳帧;播完后 class 留着无副作用(fill-mode backwards)。
 * 包装恒在(不随流式与否增删),否则轮次收束那一下整棵子树重挂,展开态 / 表单草稿全丢。
 * 子组件渲染 null 时包装随 empty:hidden 一起隐身,不在父级 flex gap 里白占一格。
 */
export default function StreamEnter({
  active,
  className,
  children,
}: {
  active: boolean;
  className?: string;
  children: ReactNode;
}) {
  const [enter] = useState(active);
  return <div className={cn('empty:hidden', enter && 'forge-stream-in', className)}>{children}</div>;
}

/**
 * 「首帧之后才出现的 key」判定:列表首帧已在的项随父级一起入场(或本就是历史),
 * 只有之后新到的项才各自播入场。收起态下到达、后来才被展开看见的项也不算新——
 * 展开是用户动作,不是新内容。
 */
export function useFreshKeys<K>(keys: readonly K[]): (key: K) => boolean {
  const known = useRef<Set<K> | null>(null);
  const prev = known.current;
  useEffect(() => {
    known.current = new Set(keys);
  });
  return (key) => prev !== null && !prev.has(key);
}
