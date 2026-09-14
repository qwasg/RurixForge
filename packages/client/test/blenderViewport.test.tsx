import { cleanup, fireEvent, render, waitFor } from '@testing-library/react';
import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import { ViewportCanvas } from '../src/components/editor/ViewportCanvas';
import { useEditorStore } from '../src/lib/editorStore';
import { forgeMock } from './forgeMock';

const stream = vi.hoisted(() => ({ sendInput: vi.fn(), close: vi.fn() }));
vi.mock('../src/lib/viewportStream', async (original) => ({
  ...await original<typeof import('../src/lib/viewportStream')>(),
  openViewportStream: () => ({ up: true, sendInput: stream.sendInput, close: stream.close,
    sendCamera: vi.fn(), sendPointer: vi.fn(), setSelected: vi.fn(), resize: vi.fn() }),
}));
beforeEach(() => {
  forgeMock.reset(); forgeMock.stubGlobal(); stream.sendInput.mockClear(); stream.close.mockClear();
  useEditorStore.setState({ entities: [], playState: 'play_running', viewportDegraded: null, camera: null });
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
});
afterEach(() => { cleanup(); delete window.forgeAPI; vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe('Blender model viewport integration', () => {
  it('releases held movement on focus loss and keeps equivalent held keys independent', async () => {
    render(<ViewportCanvas />);
    fireEvent.keyDown(window, { key: 'w', code: 'KeyW' });
    expect(stream.sendInput).toHaveBeenLastCalledWith('up', 1);
    fireEvent.keyDown(window, { key: 'ArrowUp', code: 'ArrowUp' });
    const count = stream.sendInput.mock.calls.length;
    fireEvent.keyUp(window, { key: 'w', code: 'KeyW' });
    expect(stream.sendInput).toHaveBeenCalledTimes(count);
    fireEvent.blur(window);
    expect(stream.sendInput).toHaveBeenLastCalledWith('up', 0);
  });
  it('hides the native texture child for a ModelRenderer scene so the GPU stream remains visible', async () => {
    const reportBounds = vi.fn();
    window.forgeAPI = { platform: 'win32', win: { minimize: vi.fn(), close: vi.fn(), toggleMaximize: vi.fn(), onMaximizedChanged: vi.fn() }, viewport: { reportBounds } };
    useEditorStore.setState({ entities: [{ id: 1, name: 'Model', transform: { translation: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
      components: [{ type: 'ModelRenderer', enabled: true, props: { model: 'model-guid' } }] }] });
    render(<ViewportCanvas />);
    await waitFor(() => expect(reportBounds).toHaveBeenCalledWith(expect.objectContaining({ visible: false })));
    expect(reportBounds.mock.calls.every(([arg]) => arg.visible === false)).toBe(true);
  });
});
