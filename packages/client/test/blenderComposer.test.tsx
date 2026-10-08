import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import BlenderComposer from '../src/components/studio/BlenderComposer';
import { useStudioStore, loadStudio, type StudioNode } from '../src/lib/studioStore';
import { useWorkspaceStore } from '../src/lib/workspaceStore';
import { useAssetStore } from '../src/lib/assetStore';
import type { BlenderJob } from '../../protocol/src/blender';

const mocks = vi.hoisted(() => ({ status: vi.fn(), create: vi.fn(), get: vi.fn(), open: vi.fn(), action: vi.fn() }));
vi.mock('../src/lib/blenderApi', async (original) => ({
  ...await original<typeof import('../src/lib/blenderApi')>(),
  blenderStatus: mocks.status, blenderCreateJob: mocks.create, blenderGetJob: mocks.get,
  openBlenderInCodex: mocks.open, blenderJobAction: mocks.action,
}));
const node: StudioNode = { id: 'blender-node', name: '角色', preset: 'mesh', pos: [0, 0], prompt: '带贴图和动作的角色', params: { creationMethod: 'blender', blenderKind: 'character' }, versions: [], currentVersionId: null };
const job: BlenderJob = {
  id: 'job-1', sourceId: 'source-1', workspaceId: 'a', name: '角色', prompt: node.prompt, kind: 'character',
  state: 'awaiting_codex', sourcePath: 'Sources/Blender/job-1/hero.blend', sourceBound: false, autoSync: true,
  revision: 0, createdAt: 1, updatedAt: 1, codexUrl: 'codex://threads/new?path=D%3A%2Fa&prompt=job-1', handoffPrompt: '领取 job-1',
};
function Harness() { const current = useStudioStore((s) => s.nodes[0]); return <BlenderComposer node={current} />; }
beforeEach(() => {
  localStorage.clear(); vi.clearAllMocks();
  useStudioStore.setState({ nodes: [{ ...node, params: { ...node.params } }], workspaceId: 'a', edges: [], seq: 1 });
  useWorkspaceStore.setState({ activeWorkspaceId: 'a' });
  mocks.status.mockResolvedValue({ blender: { found: true }, computerUse: { status: 'unknown' } });
  mocks.create.mockResolvedValue(job); mocks.get.mockResolvedValue(job); mocks.open.mockResolvedValue(undefined);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });
describe('Blender authoring UI', () => {
  it('durably saves the handoff before opening Codex and stays awaiting an actual claim', async () => {
    render(<Harness />);
    await waitFor(() => expect(screen.getByTestId('blender-create')).not.toBeDisabled());
    fireEvent.click(screen.getByTestId('blender-create'));
    await waitFor(() => expect(mocks.open).toHaveBeenCalledTimes(1));
    expect(mocks.create).toHaveBeenCalledWith('角色', node.prompt, 'character', 'a');
    expect(useStudioStore.getState().nodes[0].params.blenderJobId).toBe('job-1');
    expect(loadStudio('a').nodes[0].params.blenderJobId).toBe('job-1');
    expect(screen.getByText('等待 Codex 领取')).toBeInTheDocument();
    expect(useStudioStore.getState().nodes[0].versions).toHaveLength(0);
  });

  it('records only committed revisions, including source/template references, without duplicates', async () => {
    vi.spyOn(useAssetStore.getState(), 'load').mockResolvedValue(undefined);
    const published: BlenderJob = { ...job, state: 'ready', revision: 2,
      published: { modelGuid: 'model', modelPath: 'Models/model.rxmodel', prefabGuid: 'prefab', prefabPath: 'Models/template.rxprefab', revision: 2, clips: ['Idle', 'Walk'] } };
    useStudioStore.getState().recordBlenderVersion(node.id, job);
    expect(useStudioStore.getState().nodes[0].versions).toHaveLength(0);
    useStudioStore.getState().recordBlenderVersion(node.id, published);
    useStudioStore.getState().recordBlenderVersion(node.id, published);
    expect(loadStudio('a').nodes[0].versions).toHaveLength(1);
    expect(loadStudio('a').nodes[0].versions[0]).toMatchObject({ blenderJobId: 'job-1', blenderRevision: 2, guid: 'model', prefabGuid: 'prefab', animationClips: ['Idle', 'Walk'] });
  });
});
