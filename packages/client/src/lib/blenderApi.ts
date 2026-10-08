import { apiGet, apiPost, ForgeApiError, unwrapToolResult } from './forgeApi';
import { readActiveWorkspaceId } from './activeWorkspace';
import { bridge } from './bridge';
import type { BlenderAssetKind, BlenderJob, BlenderStatus } from '../../../protocol/src/blender';
export type { BlenderAssetKind, BlenderJob, BlenderStatus, BlenderJobState } from '../../../protocol/src/blender';

const ROOT = '/api/forge/blender';
export const blenderWorkspace = (): string => readActiveWorkspaceId() ?? 'default';

export function blenderStatus(workspaceId = blenderWorkspace()): Promise<BlenderStatus> {
  return apiGet(`${ROOT}/status?${new URLSearchParams({ workspaceId })}`);
}
export function blenderCreateJob(name: string, prompt: string, kind: BlenderAssetKind, workspaceId = blenderWorkspace()): Promise<BlenderJob> {
  return apiPost(`${ROOT}/jobs`, { workspaceId, name, prompt, kind });
}
export function blenderGetJob(id: string, workspaceId = blenderWorkspace()): Promise<BlenderJob> {
  return apiGet(`${ROOT}/jobs/${encodeURIComponent(id)}?${new URLSearchParams({ workspaceId })}`);
}
export function blenderJobAction(id: string, action: 'retry' | 'cancel', workspaceId = blenderWorkspace()): Promise<BlenderJob> {
  return apiPost(`${ROOT}/jobs/${encodeURIComponent(id)}/${action}`, { workspaceId });
}
export function blenderConfigure(executablePath: string, workspaceId = blenderWorkspace()): Promise<BlenderStatus> {
  return apiPost(`${ROOT}/config`, { workspaceId, executablePath });
}

export interface BlenderFrame {
  width: number;
  height: number;
  pixelsB64: string;
  deviceName?: string;
  framePath?: string;
  clips?: string[];
}

export async function blenderPreview(id: string, options: { clip?: string; time?: number; yaw?: number } = {}, workspaceId = blenderWorkspace()): Promise<BlenderFrame> {
  const response = await apiPost<unknown>(`${ROOT}/jobs/${encodeURIComponent(id)}/preview`, { workspaceId, width: 480, height: 320, ...options });
  const envelope = response as { content?: Array<{ type: string; text?: string }>; isError?: boolean };
  const value = envelope.content ? unwrapToolResult(envelope) : response;
  if (envelope.isError) throw new ForgeApiError('PREVIEW_FAILED', JSON.stringify(value));
  const frame = value as BlenderFrame;
  if (!frame || !Number.isInteger(frame.width) || !Number.isInteger(frame.height) ||
      frame.width <= 0 || frame.height <= 0 || frame.width > 2048 || frame.height > 2048 ||
      typeof frame.pixelsB64 !== 'string') {
    throw new ForgeApiError('PREVIEW_UNAVAILABLE', '引擎未返回可显示的模型画面');
  }
  return frame;
}

/** The documented deep link pre-fills a task. Opening it does not claim or start a job. */
export async function openBlenderInCodex(job: Pick<BlenderJob, 'codexUrl'>): Promise<void> {
  const url = new URL(job.codexUrl);
  if (url.protocol !== 'codex:' || url.hostname !== 'threads' || url.pathname !== '/new') {
    throw new ForgeApiError('INVALID_HANDOFF', '无效的 Codex 任务链接');
  }
  const native = bridge().codex;
  if (native) await native.openTask(url.href);
  else {
    const a = document.createElement('a');
    a.href = url.href;
    a.rel = 'noopener noreferrer';
    document.body.appendChild(a);
    a.click();
    a.remove();
  }
}
