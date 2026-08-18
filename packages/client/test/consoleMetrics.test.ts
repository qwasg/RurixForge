import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { consoleLevel, filterEvents, ringPush, typeCounts } from '@/lib/consoleUtils';
import { useEditorStore } from '@/lib/editorStore';
import { mockForgeBackend } from './forgeMock';

// ---------- consoleUtils 纯函数(F6 wave.3) ----------

describe('consoleUtils', () => {
  it('consoleLevel:playtest 行优先;error/unsupported/ok=false 为错误级;其余 info', () => {
    expect(consoleLevel({ role: 'playtest', event: 'playtest.case', ok: false })).toBe('playtest');
    expect(consoleLevel({ event: 'logic.call_error' })).toBe('error');
    expect(consoleLevel({ event: 'logic.unsupported' })).toBe('error');
    expect(consoleLevel({ event: 'playtest.report', ok: false })).toBe('error');
    expect(consoleLevel({ event: 'scene.loaded' })).toBe('info');
    expect(consoleLevel({})).toBe('info');
  });

  it('filterEvents:隐藏类型 + clearedBefore 下标裁剪', () => {
    const evs = [
      { event: 'a' },
      { event: 'b' },
      { event: 'a' },
      { event: 'c' },
    ];
    expect(filterEvents(evs, new Set(), 0)).toHaveLength(4);
    expect(filterEvents(evs, new Set(['a']), 0).map((e) => e.event)).toEqual(['b', 'c']);
    expect(filterEvents(evs, new Set(), 2).map((e) => e.event)).toEqual(['a', 'c']);
    expect(filterEvents(evs, new Set(['a']), 2).map((e) => e.event)).toEqual(['c']);
    // 清空后新到事件仍显示(clearedBefore 为快照下标)。
    const more = [...evs, { event: 'a' }];
    expect(filterEvents(more, new Set(), 4).map((e) => e.event)).toEqual(['a']);
  });

  it('typeCounts:按首现序聚合计数', () => {
    const c = typeCounts([{ event: 'a' }, { event: 'b' }, { event: 'a' }, {}]);
    expect(c).toEqual([
      { type: 'a', count: 2 },
      { type: 'b', count: 1 },
      { type: 'event', count: 1 },
    ]);
  });

  it('ringPush:追加与定长截断(cap 60)', () => {
    expect(ringPush([], 1)).toEqual([1]);
    expect(ringPush([1, 2], 3)).toEqual([1, 2, 3]);
    const full = Array.from({ length: 60 }, (_, i) => i);
    const next = ringPush(full, 60);
    expect(next).toHaveLength(60);
    expect(next[0]).toBe(1);
    expect(next[59]).toBe(60);
  });
});

// ---------- editorStore:metrics 采样环 + playtest 报告注入 ----------

const SUMMARY = {
  name: 'maze',
  entityCount: 36,
  playState: 'play_running',
  render: { frames: 100, lastTris: 7, lastNonZeroPixels: 8360 },
};

const initialState = useEditorStore.getState();
beforeEach(() => {
  useEditorStore.setState(initialState, true);
});
afterEach(() => {
  vi.unstubAllGlobals();
});

describe('editorStore F6 wave.3', () => {
  it('refreshSummary:每次成功追加三序列采样环', async () => {
    const fm = mockForgeBackend({ scene_summary: SUMMARY });
    vi.stubGlobal('fetch', fm);
    await useEditorStore.getState().refreshSummary();
    await useEditorStore.getState().refreshSummary();
    const h = useEditorStore.getState().metricsHistory;
    expect(h.frames).toEqual([100, 100]);
    expect(h.tris).toEqual([7, 7]);
    expect(h.nonZero).toEqual([8360, 8360]);
    expect(useEditorStore.getState().playState).toBe('play_running');
  });

  it('refreshSummary:采样环 cap 60(超出丢最旧)', async () => {
    useEditorStore.setState({
      metricsHistory: {
        frames: Array.from({ length: 60 }, (_, i) => i),
        tris: [],
        nonZero: [],
      },
    });
    const fm = mockForgeBackend({ scene_summary: SUMMARY });
    vi.stubGlobal('fetch', fm);
    await useEditorStore.getState().refreshSummary();
    const h = useEditorStore.getState().metricsHistory;
    expect(h.frames).toHaveLength(60);
    expect(h.frames[0]).toBe(1);
    expect(h.frames[59]).toBe(100);
  });

  it('runPlaytest:报告行注入 Console(role=playtest;红绿如实)', async () => {
    const report = {
      scene: 'projects/demo/Content/Scenes/maze.rxscene',
      ok: false,
      passed: 5,
      failed: 1,
      durationMs: 270,
      cases: [
        { name: '实体计数=36', kind: 'entity_count', pass: true, actual: 36, expected: 36, detail: '' },
        {
          name: '玩家抵达终点',
          kind: 'transform_near',
          pass: false,
          actual: [2, 0.4, 4],
          expected: [10, 0.4, 10],
          detail: 'maxDeviation=4.0, tolerance=0.3',
        },
      ],
    };
    const fm = mockForgeBackend({}, { '/api/forge/playtest/run': report });
    vi.stubGlobal('fetch', fm);
    await useEditorStore.getState().runPlaytest('tests/maze/matrix.json');
    const evs = useEditorStore.getState().events;
    expect(evs).toHaveLength(3);
    expect(evs[0].role).toBe('playtest');
    expect(evs[0].event).toBe('playtest.report');
    expect(evs[0].ok).toBe(false);
    expect(String(evs[0].summary)).toContain('FAIL 5/6');
    expect(evs[2].ok).toBe(false);
    expect(String(evs[2].summary)).toContain('玩家抵达终点');
    // playtest 报告行不遮蔽红 case:consoleLevel 对报告行归 playtest 级(着色用),ok=false 如实保留。
    expect(consoleLevel(evs[2] as Record<string, unknown>)).toBe('playtest');
  });
});
