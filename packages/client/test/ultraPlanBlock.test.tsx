import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import AssistantMessage from '@/components/chat/AssistantMessage';
import UltraPlanBlock from '@/components/chat/ultraplan/UltraPlanBlock';
import TodoTab from '@/components/workbench/TodoTab';
import { useChatStore, type ChatMsg } from '@/lib/chatStore';
import type { ChatBlock } from '@/lib/timeline';

/**
 * D-044:UltraPlan 关口卡分发 + AssistantMessage 接线 + Todo 来源标记。
 * 交互行为由各卡测试与 ultraPlanWorkflow.test.tsx 覆盖。
 */

type Ultra = Extract<ChatBlock, { kind: 'ultraplan' }>;

const card = (step: Ultra['step'], rev: number, patch: Partial<Ultra> = {}): Ultra => ({
  kind: 'ultraplan',
  step,
  upId: 'up_a1',
  rev,
  payload: {},
  ...patch,
});

function assistant(blocks: ChatBlock[], over: Partial<ChatMsg> = {}): ChatMsg {
  return {
    id: 'assistant-1',
    role: 'assistant',
    text: '',
    blocks,
    status: 'completed',
    time: '10:24',
    runId: 'run_1',
    ...over,
  };
}

afterEach(() => {
  cleanup();
});

describe('<UltraPlanBlock /> 关口卡分发', () => {
  it('五种 step 各出一张卡,带 step / rev 标记', () => {
    const blocks: Ultra[] = [
      card('questionnaire', 1, { payload: { questionnaire: { title: '塔防问卷' } } }),
      card('demo', 2, { payload: { note: '已回退到第 1 版(未重新验证)' } }),
      card('plan', 1, { payload: { name: '塔防 MVP' } }),
      card('acceptance', 3),
      card('done', 0),
    ];
    render(
      <div>
        {blocks.map((block, i) => (
          <UltraPlanBlock key={i} block={block} />
        ))}
      </div>,
    );
    const cards = screen.getAllByTestId('ultraplan-block');
    expect(cards.map((el) => el.getAttribute('data-step'))).toEqual([
      'questionnaire',
      'demo',
      'plan',
      'acceptance',
      'done',
    ]);
    expect(cards[0]).toHaveTextContent('需求问卷');
    expect(cards[0]).toHaveTextContent('塔防问卷');
    expect(cards[1]).toHaveTextContent('Demo · 第 2 版');
    expect(cards[1]).toHaveTextContent('已回退到第 1 版(未重新验证)');
    expect(cards[2]).toHaveTextContent('塔防 MVP');
    expect(cards[3]).toHaveTextContent('人工验收 · 第 3 轮');
    expect(cards[4]).toHaveTextContent('UltraPlan 已完成');
    expect(cards[1]).toHaveAttribute('data-rev', '2');
  });

  it('已提交的卡带 submitted 标记;载荷残缺不抛', () => {
    render(
      <div>
        <UltraPlanBlock block={card('questionnaire', 1, { submitted: { answers: {} } })} />
        <UltraPlanBlock block={card('plan', 1, { payload: { name: 42 } })} />
      </div>,
    );
    const cards = screen.getAllByTestId('ultraplan-block');
    expect(cards[0]).toHaveAttribute('data-submitted', '1');
    expect(cards[0]).toHaveTextContent('已提交');
    expect(cards[1]).not.toHaveAttribute('data-submitted');
  });
});

describe('<AssistantMessage /> × UltraPlan', () => {
  it('以关口卡收尾的轮不显示「模型没有返回正文」', () => {
    render(<AssistantMessage msg={assistant([card('questionnaire', 1)])} />);
    expect(screen.getByTestId('ultraplan-block')).toBeInTheDocument();
    expect(screen.queryByTestId('assistant-empty')).not.toBeInTheDocument();
  });

  it('关口卡与正文 / 过程链并存,顺序按块序', () => {
    const blocks: ChatBlock[] = [
      {
        kind: 'tool',
        toolCallId: 'c1',
        name: 'ultraplan_questionnaire',
        args: JSON.stringify({ title: '塔防问卷' }),
        ok: true,
        mcp: null,
      },
      card('questionnaire', 1),
      { kind: 'text', text: '问卷已备好。', final: true },
    ];
    render(<AssistantMessage msg={assistant(blocks)} />);
    const message = screen.getByTestId('assistant-message');
    const cardEl = screen.getByTestId('ultraplan-block');
    const textEl = screen.getByText('问卷已备好。');
    expect(message).toContainElement(cardEl);
    expect(cardEl.compareDocumentPosition(textEl) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});

describe('<TodoTab /> 来源标记', () => {
  it('source=ultraplan 的任务标「UltraPlan」并带来源说明;plan / user 文案不变', () => {
    useChatStore.setState({
      todos: [
        { id: 't1', title: '搭场景', status: 'queued', source: 'ultraplan' },
        { id: 't2', title: '计划项', status: 'queued', source: 'plan' },
        { id: 't3', title: '手动项', status: 'queued', source: 'user' },
        { id: 't4', title: '对话里建的', status: 'queued' },
      ],
    });
    render(<TodoTab />);
    expect(screen.getByTestId('todo-source-t1')).toHaveTextContent('UltraPlan');
    expect(screen.getByTestId('todo-source-t1')).toHaveAttribute('title', '来自 UltraPlan 计划');
    expect(screen.getByTestId('todo-source-t2')).toHaveAttribute('title', '来自计划文件');
    expect(screen.getByTestId('todo-source-t3')).toHaveAttribute('title', '手动创建');
    expect(screen.queryByTestId('todo-source-t4')).not.toBeInTheDocument();
  });
});
