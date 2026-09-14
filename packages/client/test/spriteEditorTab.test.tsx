import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import Workbench from '@/components/shell/Workbench';
import { useSpriteStore, type SpriteDoc } from '@/lib/spriteStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';

/**
 * F-GAME-4 精灵编辑器 tab 接线冒烟(照 storeTab/fileEditor 隔离范式):
 * openTab('sprite-editor') → tabbar 出「精灵编辑器」tab + 内容区渲染编辑器骨架;
 * store 喂 doc 后画布/右栏/预览三件套就位(不打网络,渲染面冒烟)。
 */

const initialWorkbench = useWorkbenchStore.getState();
const initialSprite = useSpriteStore.getState();

const DOC: SpriteDoc = {
  version: 1,
  texture: 'tex-1',
  pivot: [0.5, 1],
  frames: {
    frame_0: { bbox: [0, 0, 16, 16] },
    frame_1: { bbox: [16, 0, 16, 16], pivot: [0.4, 1] },
  },
  clips: { walk: { frames: ['frame_0', 'frame_1'], fps: 8, loop: true, onFinish: 'hold' } },
};

beforeEach(() => {
  useWorkbenchStore.setState(initialWorkbench, true);
  useSpriteStore.setState(initialSprite, true);
});

afterEach(() => {
  cleanup();
});

describe('精灵编辑器 tab 接线', () => {
  it('openTab(sprite-editor):tab 标题「精灵编辑器」+ 空态骨架(未打开精灵)', () => {
    render(<Workbench />);
    act(() => {
      useWorkbenchStore.getState().openTab('sprite-editor');
    });

    const tab = screen.getByTestId('workbench-tab-sprite-editor');
    expect(tab).toHaveTextContent('精灵编辑器');
    expect(screen.getByTestId('sprite-editor')).toBeInTheDocument();
    expect(screen.getByTestId('sprite-empty')).toHaveTextContent('未打开精灵');
    // 工具栏在(保存/撤销禁用态如实)。
    expect(screen.getByTestId('sprite-toolbar-save')).toBeDisabled();
    expect(screen.getByTestId('sprite-toolbar-undo')).toBeDisabled();
  });

  it('store 有 doc:画布 + 右栏(帧/clip/animator)+ 预览播放器渲染就位', () => {
    render(<Workbench />);
    act(() => {
      useWorkbenchStore.getState().openTab('sprite-editor');
      useSpriteStore.setState({
        assetPath: 'Sprites/hero.rxsprite',
        guid: 'sp-1',
        doc: DOC,
        savedJson: JSON.stringify(DOC),
        selectedFrame: 'frame_0',
        selectedClip: 'walk',
      });
    });

    expect(screen.getByTestId('sprite-canvas')).toBeInTheDocument();
    expect(screen.getByTestId('sprite-right-panel')).toBeInTheDocument();
    expect(screen.getByTestId('sprite-frame-row-frame_0')).toBeInTheDocument();
    expect(screen.getByTestId('sprite-clip-row-walk')).toBeInTheDocument();
    expect(screen.getByTestId('sprite-animator-input')).toBeInTheDocument();
    expect(screen.getByTestId('sprite-preview')).toBeInTheDocument();
    expect(screen.getByTestId('sprite-preview-play')).toBeInTheDocument();
    // 时间轴逐帧缩略图(walk 2 帧)。
    expect(screen.getByTestId('sprite-timeline-0')).toBeInTheDocument();
    expect(screen.getByTestId('sprite-timeline-1')).toBeInTheDocument();
    // 关闭 tab 恢复空 workbench(内建单例无 dirty 拦截,精灵编辑态在 store 内保留)。
    act(() => {
      useWorkbenchStore.getState().closeTab('sprite-editor');
    });
    expect(screen.queryByTestId('sprite-editor')).not.toBeInTheDocument();
  });
});
