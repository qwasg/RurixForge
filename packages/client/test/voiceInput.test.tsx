import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Composer from '@/components/chat/Composer';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { formatElapsed, joinDictation } from '@/lib/speechInput';
import { useToastStore } from '@/lib/toastStore';
import { mockForgeBackend } from './forgeMock';

/**
 * Composer 语音输入钮(上下文计量环右侧)。jsdom 无 Web Speech 实现,
 * 用假识别器桩注入 globalThis.SpeechRecognition:桩只负责「投递识别事件」,
 * 听写文本如何落进草稿、停听后补投结果如何丢弃、致命错误如何如实报——全走真实组件逻辑。
 */

interface FakeResult {
  0: { transcript: string };
  isFinal: boolean;
  length: number;
}

class FakeRecognition {
  static instances: FakeRecognition[] = [];
  lang = '';
  continuous = false;
  interimResults = false;
  maxAlternatives = 0;
  started = 0;
  stopped = 0;
  aborted = 0;
  onresult: ((e: { results: FakeResult[] }) => void) | null = null;
  onerror: ((e: { error: string }) => void) | null = null;
  onend: (() => void) | null = null;

  constructor() {
    FakeRecognition.instances.push(this);
  }

  start(): void {
    this.started += 1;
  }
  stop(): void {
    this.stopped += 1;
  }
  abort(): void {
    this.aborted += 1;
  }

  /** 投递一批识别结果(真 API 的 results 是累积列表,故每次传全量)。 */
  emit(parts: Array<[string, boolean]>): void {
    const results = parts.map(([transcript, isFinal]) => ({
      0: { transcript },
      isFinal,
      length: 1,
    }));
    act(() => this.onresult?.({ results }));
  }

  fail(error: string): void {
    act(() => this.onerror?.({ error }));
  }

  /** 识别器长静音自停(真 API 在 continuous 下也会发)。 */
  end(): void {
    act(() => this.onend?.());
  }
}

const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();

function latest(): FakeRecognition {
  const rec = FakeRecognition.instances.at(-1);
  if (!rec) throw new Error('未创建识别器');
  return rec;
}

function input(): HTMLTextAreaElement {
  return screen.getByTestId('composer-input') as HTMLTextAreaElement;
}

beforeEach(() => {
  FakeRecognition.instances = [];
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useSessionStore.setState({ activeSessionId: 'sess_1' });
  useToastStore.getState().clear();
  vi.stubGlobal('SpeechRecognition', FakeRecognition);
  vi.stubGlobal('fetch', mockForgeBackend({}, { '/api/forge/skills/list': { skills: [] } }));
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('听写拼接口径', () => {
  it('仅 ASCII 相邻才补空格;中文直接相接', () => {
    expect(joinDictation('', '你好')).toBe('你好');
    expect(joinDictation('把箱子', '挪到原点')).toBe('把箱子挪到原点');
    expect(joinDictation('open', 'file')).toBe('open file');
    expect(joinDictation('打开 ', 'main.rs')).toBe('打开 main.rs');
    expect(joinDictation('你好，', 'world')).toBe('你好，world');
  });

  it('已录时长按 m:ss 显示', () => {
    expect(formatElapsed(0)).toBe('0:00');
    expect(formatElapsed(9)).toBe('0:09');
    expect(formatElapsed(95)).toBe('1:35');
  });
});

describe('<Composer /> 语音输入钮', () => {
  it('落在工具行上下文计量环右侧;空闲态不显时长', () => {
    render(<Composer />);
    const voice = screen.getByTestId('composer-voice');
    expect(screen.getByTestId('composer-tools')).toContainElement(voice);
    expect(screen.getByTestId('composer-context').compareDocumentPosition(voice)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(voice).toHaveAttribute('aria-pressed', 'false');
    expect(screen.queryByTestId('composer-voice-elapsed')).not.toBeInTheDocument();
  });

  it('点开听:连续 + 实时片段,未定稿边说边刷,定稿后落定并接在草稿后', () => {
    render(<Composer />);
    fireEvent.change(input(), { target: { value: '帮我' } });
    fireEvent.click(screen.getByTestId('composer-voice'));

    const rec = latest();
    expect(rec.started).toBe(1);
    expect(rec.continuous).toBe(true);
    expect(rec.interimResults).toBe(true);
    expect(rec.lang).toBe('zh-CN');
    expect(screen.getByTestId('composer-voice')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('composer-voice-elapsed')).toHaveTextContent('0:00');

    rec.emit([['把箱子', false]]);
    expect(input().value).toBe('帮我把箱子');
    // 未定稿片段被后续结果整体替换,不叠加
    rec.emit([['把箱子挪到', false]]);
    expect(input().value).toBe('帮我把箱子挪到');
    rec.emit([
      ['把箱子挪到原点。', true],
      ['再', false],
    ]);
    expect(input().value).toBe('帮我把箱子挪到原点。再');
  });

  it('再点停听;停后补投的定稿结果不再写回草稿', () => {
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-voice'));
    const rec = latest();
    rec.emit([['你好', false]]);
    expect(input().value).toBe('你好');

    fireEvent.click(screen.getByTestId('composer-voice'));
    expect(rec.stopped).toBe(1);
    expect(screen.getByTestId('composer-voice')).toHaveAttribute('aria-pressed', 'false');
    rec.emit([['你好世界', true]]);
    expect(input().value).toBe('你好');
  });

  it('长静音自停会自动重开,已定稿文本跨识别器保留', () => {
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-voice'));
    const rec = latest();
    rec.emit([['第一段。', true]]);
    rec.end();
    expect(rec.started).toBe(2); // 复用同一识别器重开
    expect(screen.getByTestId('composer-voice')).toHaveAttribute('aria-pressed', 'true');

    rec.emit([['第二段', false]]);
    expect(input().value).toBe('第一段。第二段');
  });

  it('听写途中手打:以输入框新值为底稿续听,已注入部分不重复', () => {
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-voice'));
    const rec = latest();
    rec.emit([['你好', true]]);
    expect(input().value).toBe('你好');

    fireEvent.change(input(), { target: { value: '你好，' } });
    rec.emit([
      ['你好', true],
      ['世界', false],
    ]);
    expect(input().value).toBe('你好，世界');
  });

  it('致命错误如实弹 toast 并停听;no-speech 不打断', () => {
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-voice'));
    const rec = latest();

    rec.fail('no-speech');
    expect(screen.getByTestId('composer-voice')).toHaveAttribute('aria-pressed', 'true');
    expect(useToastStore.getState().items).toHaveLength(0);

    rec.fail('network');
    expect(screen.getByTestId('composer-voice')).toHaveAttribute('aria-pressed', 'false');
    const [toast] = useToastStore.getState().items;
    expect(toast.kind).toBe('error');
    expect(toast.title).toContain('Chrome');
  });

  it('发送前自动停听', async () => {
    useChatStore.setState({ sendMessage: vi.fn().mockResolvedValue(true) });
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-voice'));
    const rec = latest();
    rec.emit([['说一句你好', true]]);
    fireEvent.click(screen.getByTestId('composer-send'));
    expect(rec.stopped).toBe(1);
    expect(screen.getByTestId('composer-voice')).toHaveAttribute('aria-pressed', 'false');
    await act(async () => Promise.resolve());
    expect(input().value).toBe('');
  });

  it('无 Web Speech 构造器:钮禁用并如实说明原因', () => {
    vi.stubGlobal('SpeechRecognition', undefined);
    render(<Composer />);
    const voice = screen.getByTestId('composer-voice');
    expect(voice).toBeDisabled();
    expect(voice).toHaveAttribute('title', expect.stringContaining('不支持语音输入'));
    fireEvent.click(voice);
    expect(FakeRecognition.instances).toHaveLength(0);
  });
});
