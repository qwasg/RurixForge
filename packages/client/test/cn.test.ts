import { describe, expect, it } from 'vitest';
import { cn } from '@/lib/cn';

describe('cn', () => {
  it('合并普通类与条件类(clsx 行为)', () => {
    expect(cn('a', 'b')).toBe('a b');
    expect(cn('a', false && 'b', 'c')).toBe('a c');
    expect(cn('a', undefined, null, 'b')).toBe('a b');
  });

  it('tailwind-merge 冲突合并:同组类后者覆盖前者', () => {
    expect(cn('px-2', 'px-4')).toBe('px-4');
    expect(cn('text-ink', 'text-muted')).toBe('text-muted');
    expect(cn('rounded-md', 'rounded-xl')).toBe('rounded-xl');
  });

  it('非冲突类全部保留', () => {
    expect(cn('px-2', 'py-3', 'text-sm')).toBe('px-2 py-3 text-sm');
  });
});
