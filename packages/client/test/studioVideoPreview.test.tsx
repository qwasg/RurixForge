import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import StudioPreview from '@/components/studio/StudioPreview';
import { apiGenVideoFileUrl } from '@/lib/forgeApi';
import { useStudioStore, type StudioVersion } from '@/lib/studioStore';

const fileRef = '.forge/tmp/gen/视频 & preview.mp4';
const version: StudioVersion = { id: 'v1', createdAt: 1, backendId: 'comfyui-minimax-h3', prompt: '海边', fileRef, mime: 'video/mp4' };

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem('forge:activeWorkspace', 'other-active');
  useStudioStore.setState({ workspaceId: 'board & one' });
});

afterEach(() => {
  cleanup();
  localStorage.clear();
});

describe('已保存视频预览', () => {
  it.each(['video', 'sprite'] as const)('%s 重开后通过当前画板的文件 URL 播放,不受其他活动项目影响', (kind) => {
    render(<StudioPreview kind={kind} version={kind === 'sprite'
      ? { ...version, fileRef: '.forge/tmp/gen/atlas.png', videoFileRef: fileRef } : version} />);
    const player = screen.getByTestId('studio-video-player');
    const src = new URL(player.getAttribute('src')!, 'http://localhost');
    expect(src.pathname).toBe('/api/forge/gen/video/file');
    expect(src.searchParams.get('fileRef')).toBe(fileRef);
    expect(src.searchParams.get('workspaceId')).toBe('board & one');
    expect(player).toHaveAttribute('controls');
    expect(player).toHaveAttribute('preload', 'metadata');
    expect(player).not.toHaveAttribute('autoplay');
  });

  it('会话视频 dataUrl 优先于持久化 fileRef', () => {
    const dataUrl = 'data:video/mp4;base64,AAAA';
    render(<StudioPreview kind="video" version={{ ...version, dataUrl }} />);
    expect(screen.getByTestId('studio-video-player')).toHaveAttribute('src', dataUrl);
  });

  it('小卡片仍只显示预览占位,不加载视频控件', () => {
    render(<StudioPreview kind="video" version={version} compact />);
    expect(screen.queryByTestId('studio-video-player')).toBeNull();
  });

  it.each([undefined, '', 'Content/video.mp4', '../video.mp4', 'https://example.com/video.mp4',
    '.forge/tmp/gen/../video.mp4', '.forge/tmp/gen/atlas.png'])('缺失或非法引用 %s 保持占位', (invalid) => {
    render(<StudioPreview kind="video" version={{ ...version, fileRef: invalid }} />);
    expect(screen.queryByTestId('studio-video-player')).toBeNull();
    expect(apiGenVideoFileUrl(invalid)).toBeUndefined();
  });

  it('文件缺失或解码失败时如实提示,切换另一产物后可重新加载', () => {
    const { rerender } = render(<StudioPreview kind="video" version={version} />);
    fireEvent.error(screen.getByTestId('studio-video-player'));
    expect(screen.getByTestId('studio-video-unavailable')).toHaveTextContent('视频文件暂不可用');
    expect(screen.getByTestId('studio-video-unavailable')).toHaveTextContent('视频 & preview.mp4');
    rerender(<StudioPreview kind="video" version={{ ...version, id: 'v2', fileRef: '.forge/tmp/gen/next.mp4' }} />);
    expect(screen.queryByTestId('studio-video-unavailable')).toBeNull();
    expect(screen.getByTestId('studio-video-player')).toBeInTheDocument();
  });

  it('URL helper 的省略工作区取活动项目,显式 null 始终指向默认项目', () => {
    const selected = new URL(apiGenVideoFileUrl(fileRef)!, 'http://localhost');
    const explicit = new URL(apiGenVideoFileUrl(fileRef, 'named project')!, 'http://localhost');
    const fallback = new URL(apiGenVideoFileUrl(fileRef, null)!, 'http://localhost');
    expect(selected.searchParams.get('workspaceId')).toBe('other-active');
    expect(explicit.searchParams.get('workspaceId')).toBe('named project');
    expect(fallback.searchParams.has('workspaceId')).toBe(false);
    expect(fallback.searchParams.get('fileRef')).toBe(fileRef);
  });
});
