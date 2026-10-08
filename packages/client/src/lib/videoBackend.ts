import type { GenBackendInfo } from './genStore';

export const LOCAL_H3_BACKEND_ID = 'comfyui-minimax-h3';

export function genBackendLabel(id: string, label?: string): string {
  const labels: Record<string, string> = {
    'remote-openai-compatible': 'OpenAI 兼容图像',
    'local-mock': '本地示例图像',
    [LOCAL_H3_BACKEND_ID]: 'MiniMax H3 · 本地',
    'xzapi-video': 'XZAPI 视频',
    'aliyun-minimax-video': '阿里云 MiniMax 视频',
    'remote-video-compatible': '兼容视频接口',
    'remote-audio-compatible': '兼容音频接口',
    meshy: 'Meshy 3D',
    'remote-mesh-compatible': '兼容 3D 接口',
  };
  return label || labels[id] || id;
}

export function supportsGenKind(backend: GenBackendInfo, kind: string): boolean {
  const kinds = backend.capabilities?.kinds;
  return Array.isArray(kinds) && kinds.includes(kind);
}

export function resolveVideoBackend(
  backends: GenBackendInfo[], params: Record<string, unknown>, kind: 'text2video' | 'image2video',
): GenBackendInfo | undefined {
  const compatible = backends.filter((b) => supportsGenKind(b, kind));
  return typeof params.backend === 'string' && params.backend !== ''
    ? compatible.find((b) => b.id === params.backend)
    : compatible.find((b) => b.configured);
}

function strings(value: unknown, fallback: string[]): string[] {
  const valid = Array.isArray(value) ? value.filter((x): x is string => typeof x === 'string' && x !== '') : [];
  return valid.length > 0 ? valid : fallback;
}

/** Composer 与画板直接生成共用能力参数,避免未打开详情时仍发送旧的 720p/5s。 */
export function videoBackendOptions(backend: GenBackendInfo | undefined, params: Record<string, unknown>, reset = false) {
  const caps = backend?.capabilities;
  const aspects = strings(caps?.aspects, ['16:9', '9:16', '1:1']);
  const resolutions = strings(caps?.resolutions, ['720p', '1080p', '2k']);
  const defaultResolution = typeof caps?.defaultResolution === 'string' && resolutions.includes(caps.defaultResolution)
    ? caps.defaultResolution : resolutions[0];
  const min = caps?.minDurationSec;
  const max = caps?.maxDurationSec;
  const durations = typeof min === 'number' && Number.isInteger(min) && typeof max === 'number'
    && Number.isInteger(max) && min > 0 && max >= min && max - min <= 120
    ? Array.from({ length: max - min + 1 }, (_, i) => min + i) : [5, 10];
  const hasDefaultDuration = typeof caps?.defaultDurationSec === 'number' && durations.includes(caps.defaultDurationSec);
  const defaultDuration = hasDefaultDuration ? caps!.defaultDurationSec as number : durations.includes(5) ? 5 : durations[0];
  const validResolution = typeof params.resolution === 'string' && resolutions.includes(params.resolution);
  // 新草稿/旧云端草稿进入本地预览档时一起采用短时长,已有合法本地参数照常保留。
  const resetDuration = reset || (hasDefaultDuration && !validResolution);
  return {
    aspects, resolutions, durations,
    aspect: typeof params.aspect === 'string' && aspects.includes(params.aspect) ? params.aspect : aspects[0],
    resolution: !reset && validResolution ? params.resolution as string : defaultResolution,
    durationSec: !resetDuration && typeof params.durationSec === 'number' && durations.includes(params.durationSec)
      ? params.durationSec : defaultDuration,
  };
}
