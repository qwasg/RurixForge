import { describe, expect, it } from 'vitest';
import {
  isPlanPath,
  parsePlanFile,
  planNameFromPath,
  planTodoProgress,
  PLAN_DIR,
} from '@/lib/planFile';

/**
 * D-035 计划文件解析:与 agentd plan_doc.rs 的写出形态对拍。
 * serde_yaml 的列表项不缩进、必要时才加引号,手写文件常带缩进——两种都必须认。
 */

const SERDE_YAML_FORM = `---
name: 敌人波次系统
overview: 分三波刷怪
todos:
- id: wave-config
  content: 新增 WaveConfig 组件
  status: pending
- id: spawner
  content: 写生成器脚本
  status: pending
---

# 敌人波次系统

## 现状
crates/forge-scene/src/lib.rs:20 无波次概念。
`;

const INDENTED_FORM = `---
name: 缩进写法
overview: ''
todos:
  - id: a
    content: 甲
    status: completed
---
正文
`;

describe('parsePlanFile', () => {
  it('serde_yaml 形态:name/overview/todos/正文全解析', () => {
    const p = parsePlanFile(SERDE_YAML_FORM, `${PLAN_DIR}/敌人波次系统.plan.md`);
    expect(p.error).toBeUndefined();
    expect(p.name).toBe('敌人波次系统');
    expect(p.overview).toBe('分三波刷怪');
    expect(p.todos).toEqual([
      { id: 'wave-config', content: '新增 WaveConfig 组件', status: 'pending' },
      { id: 'spawner', content: '写生成器脚本', status: 'pending' },
    ]);
    expect(p.body).toContain('## 现状');
    expect(p.body.startsWith('# 敌人波次系统')).toBe(true);
  });

  it('缩进列表 + 单引号空串同样认', () => {
    const p = parsePlanFile(INDENTED_FORM);
    expect(p.error).toBeUndefined();
    expect(p.name).toBe('缩进写法');
    expect(p.overview).toBe('');
    expect(p.todos).toEqual([{ id: 'a', content: '甲', status: 'completed' }]);
    expect(p.body).toBe('正文');
  });

  it('YAML 引号与转义:冒号/引号/井号原样还原', () => {
    const text = `---
name: "计划: 带冒号 \\"引号\\""
overview: '别人的 '' 单引号'
todos:
- id: x
  content: "改 foo: bar 字段"
---
b`;
    const p = parsePlanFile(text);
    expect(p.name).toBe('计划: 带冒号 "引号"');
    expect(p.overview).toBe("别人的 ' 单引号");
    expect(p.todos[0].content).toBe('改 foo: bar 字段');
  });

  it('CRLF 与 BOM 容忍', () => {
    const p = parsePlanFile(`\uFEFF---\r\nname: x\r\ntodos:\r\n- id: a\r\n  content: 甲\r\n---\r\n正文\r\n`);
    expect(p.error).toBeUndefined();
    expect(p.name).toBe('x');
    expect(p.todos).toHaveLength(1);
    expect(p.body).toBe('正文');
  });

  it('front matter 损坏:如实报 error 且正文仍可读,名字回落文件名', () => {
    const noFm = parsePlanFile('# 只是一篇 markdown', `${PLAN_DIR}/救火.plan.md`);
    expect(noFm.error).toContain('起始');
    expect(noFm.body).toBe('# 只是一篇 markdown');
    expect(noFm.name).toBe('救火');

    expect(parsePlanFile('---\nname: x\n').error).toContain('结束');

    const noName = parsePlanFile('---\noverview: 有概述没名字\n---\n正文', `${PLAN_DIR}/兜底.plan.md`);
    expect(noName.error).toContain('name');
    expect(noName.body).toBe('正文');
    expect(noName.name).toBe('兜底');
  });

  it('缺 id 或 content 的条目丢弃(不造假条目)', () => {
    const p = parsePlanFile('---\nname: x\ntodos:\n- id: a\n- content: 无 id\n- id: c\n  content: 丙\n---\nb');
    expect(p.todos).toEqual([{ id: 'c', content: '丙', status: 'pending' }]);
  });

  it('todos 之后的顶层键不被当成待办字段', () => {
    const p = parsePlanFile('---\ntodos:\n- id: a\n  content: 甲\nname: 后置名字\n---\nb');
    expect(p.name).toBe('后置名字');
    expect(p.todos).toEqual([{ id: 'a', content: '甲', status: 'pending' }]);
  });
});

describe('isPlanPath', () => {
  it('只认 .forge/plans 下单层 .plan.md', () => {
    expect(isPlanPath('.forge/plans/x.plan.md')).toBe(true);
    expect(isPlanPath('.forge\\plans\\x.plan.md')).toBe(true);
    expect(isPlanPath('.forge/plans/sub/x.plan.md')).toBe(false);
    expect(isPlanPath('.forge/plans/../../secret.plan.md')).toBe(false);
    expect(isPlanPath('.forge/plans/x.md')).toBe(false);
    expect(isPlanPath('.forge/plans/.plan.md')).toBe(false);
    expect(isPlanPath('plans/x.plan.md')).toBe(false);
  });
});

describe('planNameFromPath / planTodoProgress', () => {
  it('路径 → 回落名(去目录与 .plan.md)', () => {
    expect(planNameFromPath('.forge/plans/敌人波次.plan.md')).toBe('敌人波次');
    expect(planNameFromPath('x.plan.md')).toBe('x');
  });

  it('进度优先取会话待办实时状态,未物化回落文件 status', () => {
    const todos = [
      { id: 'a', content: '甲', status: 'pending' },
      { id: 'b', content: '乙', status: 'completed' },
      { id: 'c', content: '丙', status: 'pending' },
    ];
    // a 已在会话里跑完(文件里还写 pending)→ 计 done;c 无映射 → 用文件的 pending。
    const live = new Map([['a', 'completed']]);
    expect(planTodoProgress(todos, (id) => live.get(id))).toEqual({ done: 2, total: 3 });
  });
});
