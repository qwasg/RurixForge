import { useEffect } from 'react';
import { apiGet } from './forgeApi';
import { useWorkspaceStore } from './workspaceStore';
import { useStudioStore } from './studioStore';
import { useAssetStore } from './assetStore';
import { useEditorStore } from './editorStore';
import type { BlenderJob } from '../../../protocol/src/blender';

/** Follow server revisions even when the creation panel is closed or Codex authored externally. */
export function useBlenderSync(): void {
  const workspaceId = useWorkspaceStore((s) => s.activeWorkspaceId) ?? 'default';
  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const revisions = new Map<string, number>();
    const poll = async () => {
      let delay = 5000;
      try {
        const result = await apiGet<{ jobs: BlenderJob[] }>(`/api/forge/blender/jobs?${new URLSearchParams({ workspaceId })}`);
        if (!live) return;
        let changed = false;
        for (const job of result.jobs ?? []) {
          if (!job.published || job.revision === revisions.get(job.id)) continue;
          revisions.set(job.id, job.revision);
          changed = true;
          const studio = useStudioStore.getState();
          if ((studio.workspaceId ?? 'default') === workspaceId) {
            for (const node of studio.nodes) {
              if (node.params.blenderJobId === job.id) studio.recordBlenderVersion(node.id, job);
            }
          }
        }
        if (changed) {
          useAssetStore.setState({ thumbs: {} });
          await useAssetStore.getState().load();
          if (live) await useEditorStore.getState().loadEntities();
        }
      } catch {
        // Backend may be restarting; the creation panel displays actionable diagnostics.
        delay = 15000;
      } finally { if (live) timer = setTimeout(() => void poll(), delay); }
    };
    void poll();
    return () => { live = false; clearTimeout(timer); };
  }, [workspaceId]);
}
