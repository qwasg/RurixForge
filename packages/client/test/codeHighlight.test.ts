import { describe, expect, it } from 'vitest';
import { highlightCode, supportsHighlight } from '@/lib/codeHighlight';

describe('highlightCode(对话代码块静态高亮)', () => {
  it('ts:关键字/字符串/注释各有 token 类,行数与原文一致', async () => {
    const lines = await highlightCode('const a = "x";\n// note\n', 'ts');
    expect(lines).not.toBeNull();
    expect(lines).toHaveLength(3);
    const spans = lines!.flat();
    expect(spans.some((s) => s.text === 'const' && s.cls?.includes('tok-keyword'))).toBe(true);
    expect(spans.some((s) => s.cls?.includes('tok-string'))).toBe(true);
    expect(lines![1].some((s) => s.cls?.includes('tok-comment'))).toBe(true);
    expect(lines!.map((l) => l.map((s) => s.text).join('')).join('\n')).toBe('const a = "x";\n// note\n');
  });

  it('rust / json 别名可用;未知语言与超长代码回落 null', async () => {
    expect(supportsHighlight('rs')).toBe(true);
    expect(supportsHighlight('JSON')).toBe(true);
    expect(await highlightCode('x', 'brainfuck')).toBeNull();
    expect(await highlightCode('a'.repeat(20_001), 'ts')).toBeNull();
    const json = await highlightCode('{"k": 1}', 'json');
    expect(json!.flat().some((s) => s.cls?.includes('tok-number'))).toBe(true);
  });
});
