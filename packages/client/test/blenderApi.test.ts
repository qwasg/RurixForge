import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { blenderCreateJob, blenderGetJob, blenderPreview, openBlenderInCodex } from '../src/lib/blenderApi';
import { writeActiveWorkspaceId } from '../src/lib/activeWorkspace';

describe('Blender bridge client contract', () => {
  const fetcher = vi.fn();
  beforeEach(() => { localStorage.clear(); vi.stubGlobal('fetch', fetcher); fetcher.mockReset(); });
  afterEach(() => { vi.unstubAllGlobals(); delete window.forgeAPI; });

  it('scopes creation and subsequent polls to the original workspace', async () => {
    writeActiveWorkspaceId('project-a');
    fetcher.mockResolvedValue(new Response(JSON.stringify({ id: 'job-1', state: 'awaiting_codex' }), { status: 200 }));
    const job = await blenderCreateJob('角色', '制作带待机与行走的角色', 'character', 'project-a');
    expect(job.state).toBe('awaiting_codex');
    expect(JSON.parse(fetcher.mock.calls[0][1].body)).toEqual({ workspaceId: 'project-a', name: '角色', prompt: '制作带待机与行走的角色', kind: 'character' });
    writeActiveWorkspaceId('project-b');
    fetcher.mockResolvedValue(new Response(JSON.stringify(job), { status: 200 }));
    await blenderGetJob('job-1', 'project-a');
    expect(fetcher.mock.calls[1][0]).toBe('/api/forge/blender/jobs/job-1?workspaceId=project-a');
  });

  it('decodes actual engine preview envelopes and surfaces a render failure', async () => {
    const frame = { width: 1, height: 1, pixelsB64: '/////w==', deviceName: 'test-device' };
    fetcher.mockResolvedValueOnce(new Response(JSON.stringify({ content: [{ type: 'text', text: JSON.stringify(frame) }] }), { status: 200 }));
    expect(await blenderPreview('job-1', { clip: 'Idle', time: 0.5, yaw: 1 }, 'a')).toEqual(frame);
    expect(JSON.parse(fetcher.mock.calls[0][1].body)).toMatchObject({ workspaceId: 'a', clip: 'Idle', time: 0.5, yaw: 1 });
    fetcher.mockResolvedValueOnce(new Response(JSON.stringify({ isError: true, content: [{ type: 'text', text: 'DEV_ENV_DEGRADE' }] }), { status: 200 }));
    await expect(blenderPreview('job-1')).rejects.toThrow('DEV_ENV_DEGRADE');
  });

  it('opens the official native handoff without submitting or claiming the job', async () => {
    const openTask = vi.fn().mockResolvedValue(undefined);
    window.forgeAPI = { codex: { openTask }, platform: 'win32', win: { minimize: vi.fn(), toggleMaximize: vi.fn(), close: vi.fn(), onMaximizedChanged: vi.fn() } };
    const url = 'codex://threads/new?path=D%3A%2FProject&prompt=job-1';
    await openBlenderInCodex({ codexUrl: url });
    expect(openTask).toHaveBeenCalledWith(url);
    expect(fetcher).not.toHaveBeenCalled();
    await expect(openBlenderInCodex({ codexUrl: 'https://example.com' })).rejects.toThrow('无效');
  });
});
