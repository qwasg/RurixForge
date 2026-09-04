import { useEffect, useRef, useState } from 'react';
import { ArrowUp, Check, ChevronDown, Cpu, FolderTree, Image as ImageIcon, Loader2, Settings2, Square } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useAssetStore } from '@/lib/assetStore';
import { apiFfmpegStatus, type FfmpegStatus } from '@/lib/forgeApi';
import { configuredBackends, useGenStore, type GenBackendInfo } from '@/lib/genStore';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSettingsStore } from '@/lib/settingsStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import {
  presetOf,
  TEXT_TEMPLATES,
  useStudioStore,
  type StudioNode,
} from '@/lib/studioStore';

/**
 * 素材创作生成输入条(详情画布底部悬浮;参考即梦式节点输入条形态):
 * prompt textarea(n/7500 计数)+ 左下模型选择弹层 + 各通道参数 chips + 右侧发送圆钮。
 * text:模板 chips(自由编写/大纲/地图草稿/剧本/策划案/提示词),LLM 渠道由后端
 * resolve_provider 自动解析(deepseek/mock),模型钮如实显示不可切;
 * image:尺寸(256/512/1024)+ 数量(×1..4);video:比例/分辨率/时长;
 * audio:「音频生成/音乐生成」子 tab + 音色/格式/歌词/纯音乐开关。
 * 后端选择:GET /api/forge/gen/backends 按 capabilities.kinds 过滤;未配置项如实标注,
 * 生成时报 GEN_BACKEND_NOT_CONFIGURED → 错误条附「去设置」跳设置·模型页。
 */

const PROMPT_MAX = 7500;

const chipBtn =
  'rounded border border-edge-strong px-1.5 py-px text-[10px] text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2';
const chipActive = 'border-acc bg-shell-active text-fg';

/** 后端是否支持某能力(capabilities.kinds 数组;缺省 false)。 */
function supportsKind(b: GenBackendInfo, kind: string): boolean {
  const kinds = b.capabilities?.kinds;
  return Array.isArray(kinds) && kinds.some((k) => k === kind);
}

/** 节点媒介 → gen 后端能力字符串(text 无 gen 后端面)。 */
function genKindOf(node: StudioNode): string | null {
  const preset = presetOf(node.preset);
  if (!preset) return null;
  switch (preset.kind) {
    case 'image':
      return 'text2img';
    case 'model':
      return 'text2mesh';
    case 'video':
      return 'text2video';
    case 'sprite':
      // 角色动画必带参考图,能力面要的是图生视频而非文生视频。
      return 'image2video';
    case 'audio':
      return node.params.mode === 'music' ? 'music' : 'tts';
    default:
      return null;
  }
}

// ---------- 模型选择弹层 ----------

function ModelPicker({ node, onClose }: { node: StudioNode; onClose: () => void }) {
  const backends = useGenStore((s) => s.backends);
  const backendsError = useGenStore((s) => s.backendsError);
  const setParam = useStudioStore((s) => s.setParam);
  const ref = useRef<HTMLDivElement>(null);
  const kind = genKindOf(node);
  const list = kind === null ? [] : backends.filter((b) => supportsKind(b, kind));
  const selected = typeof node.params.backend === 'string' ? node.params.backend : '';

  useEffect(() => {
    const onDown = (ev: MouseEvent) => {
      if (!ref.current?.contains(ev.target as Node)) onClose();
    };
    document.addEventListener('mousedown', onDown, true);
    return () => document.removeEventListener('mousedown', onDown, true);
  }, [onClose]);

  return (
    <div
      ref={ref}
      data-testid="studio-model-picker"
      className="absolute bottom-full left-0 z-30 mb-1 w-[240px] rounded-lg border border-edge-strong bg-shell-float p-1 shadow-pop"
    >
      <p className="px-1.5 pb-1 pt-0.5 text-[10px] text-fg-4">模型</p>
      {kind === null ? (
        <p className="px-1.5 pb-1 text-2xs leading-4 text-fg-3">
          文本通道由 LLM 渠道自动解析(DeepSeek 已配 key 时走 DeepSeek,否则本地 Mock);
          渠道与密钥在设置·模型页管理。
        </p>
      ) : (
        <>
          <button
            type="button"
            data-testid="studio-model-auto"
            onClick={() => {
              setParam(node.id, 'backend', '');
              onClose();
            }}
            className="flex w-full items-center gap-1.5 rounded px-1.5 py-1 text-left transition-colors hover:bg-shell-hover"
          >
            <span className="min-w-0 flex-1">
              <span className="block truncate text-2xs text-fg">自动</span>
              <span className="block text-[10px] text-fg-4">取首个已配置的后端</span>
            </span>
            {selected === '' && <Check size={12} strokeWidth={2} className="shrink-0 text-acc" />}
          </button>
          {list.map((b) => (
            <button
              key={b.id}
              type="button"
              data-testid={`studio-model-opt-${b.id}`}
              onClick={() => {
                setParam(node.id, 'backend', b.id);
                onClose();
              }}
              className="flex w-full items-center gap-1.5 rounded px-1.5 py-1 text-left transition-colors hover:bg-shell-hover"
            >
              <span className="min-w-0 flex-1">
                <span className="block truncate font-mono text-2xs text-fg">{b.id}</span>
                <span className={cn('block text-[10px]', b.configured ? 'text-sage' : 'text-fg-4')}>
                  {b.configured ? '已配置' : '未配置(生成时如实报错,去设置页配 endpoint/key)'}
                </span>
              </span>
              {selected === b.id && <Check size={12} strokeWidth={2} className="shrink-0 text-acc" />}
            </button>
          ))}
          {list.length === 0 && (
            <p className="px-1.5 pb-1 text-2xs text-fg-4">
              {backendsError !== null
                ? `后端清单拉取失败:${backendsError}(可关闭后在输入条上方点重试)`
                : '无支持该能力的后端条目'}
            </p>
          )}
        </>
      )}
    </div>
  );
}

// ---------- 输入条 ----------

export default function StudioComposer({ node }: { node: StudioNode }) {
  const setPrompt = useStudioStore((s) => s.setPrompt);
  const setParam = useStudioStore((s) => s.setParam);
  const generate = useStudioStore((s) => s.generate);
  const cancelGenerate = useStudioStore((s) => s.cancelGenerate);
  const resolvePermission = useStudioStore((s) => s.resolvePermission);
  const busy = useStudioStore((s) => s.busyIds.includes(node.id));
  const lastError = useStudioStore((s) => s.lastError);
  const clearError = useStudioStore((s) => s.clearError);
  const run = useStudioStore((s) => s.nodeRuns[node.id]);
  const pending = useStudioStore((s) =>
    s.pendingPermission?.nodeId === node.id ? s.pendingPermission : null,
  );
  const readonlyWorkspaceIds = useStudioStore((s) => s.readonlyWorkspaceIds);
  const setReadonlyWorkspaceIds = useStudioStore((s) => s.setReadonlyWorkspaceIds);
  const includeLibrary = useStudioStore((s) => s.includeLibrary);
  const workspaces = useWorkspaceStore((s) => s.workspaces);
  const activeWorkspaceId = useWorkspaceStore((s) => s.activeWorkspaceId);
  const loadWorkspaces = useWorkspaceStore((s) => s.loadAll);
  const [scopeOpen, setScopeOpen] = useState(false);
  const backends = useGenStore((s) => s.backends);
  const backendsLoaded = useGenStore((s) => s.backendsLoaded);
  const backendsError = useGenStore((s) => s.backendsError);
  const loadBackends = useGenStore((s) => s.loadBackends);
  const [pickerOpen, setPickerOpen] = useState(false);
  const assets = useAssetStore((s) => s.items);
  const loadAssets = useAssetStore((s) => s.load);
  const [ffmpeg, setFfmpeg] = useState<FfmpegStatus | null>(null);
  const isSprite = presetOf(node.preset)?.kind === 'sprite';
  /** 上游连过来的原画节点(有则参考图自动取它的当前版本,无需手选) */
  const upstreamRef = useStudioStore((s) => {
    if (!isSprite) return null;
    for (const e of s.edges.filter((e) => e.to === node.id)) {
      const up = s.nodes.find((n) => n.id === e.from);
      if (up === undefined || presetOf(up.preset)?.kind !== 'image') continue;
      const cur = up.versions.find((v) => v.id === up.currentVersionId);
      if (cur?.assetPath !== undefined || cur?.fileRef !== undefined) return up.name;
    }
    return null;
  });

  // 进入详情画布即刷新后端清单(设置页配置后切回来,configured 状态不吃陈旧缓存)
  useEffect(() => {
    void loadBackends();
    void loadWorkspaces();
  }, [node.id, loadBackends, loadWorkspaces]);

  // 角色动画额外要两样:能选的贴图资产,以及 ffmpeg 到底在不在(截帧全靠它)。
  useEffect(() => {
    if (!isSprite) return;
    void loadAssets();
    apiFfmpegStatus().then(setFfmpeg, () => setFfmpeg({ found: false }));
  }, [isSprite, node.id, loadAssets]);

  const preset = presetOf(node.preset);
  if (!preset) return null;
  const kind = preset.kind;
  const genKind = genKindOf(node);
  const error = lastError !== null && lastError.nodeId === node.id ? lastError : null;

  const selectedBackend = typeof node.params.backend === 'string' ? node.params.backend : '';
  const kindBackends = genKind === null ? [] : backends.filter((b) => supportsKind(b, genKind));
  const anyConfigured = configuredBackends(kindBackends).length > 0;
  /** 模型钮显示文案(text = LLM 自动;其余 = 显式选择或「自动」) */
  const modelLabel =
    kind === 'text' ? 'LLM · 自动' : selectedBackend !== '' ? selectedBackend : '自动';

  const openModelsSettings = () => {
    useSettingsStore.getState().setPage('models');
    useOverlayStore.getState().open('settings');
  };

  const send = () => {
    if (busy || node.prompt.trim() === '') return;
    void generate(node.id);
  };

  const templateId = typeof node.params.template === 'string' ? node.params.template : 'free';
  const audioMode = node.params.mode === 'music' ? 'music' : 'tts';

  return (
    <div
      data-testid="studio-composer"
      className="pointer-events-auto w-[560px] max-w-[94%]"
    >
      {/* 错误条:错误码如实;NOT_CONFIGURED 引导设置页 */}
      {error !== null && (
        <div
          data-testid="studio-composer-error"
          className="mb-1.5 flex items-start gap-2 rounded-lg border border-danger/50 bg-shell-panel px-2.5 py-1.5 shadow-composer"
        >
          <p className="min-w-0 flex-1 text-2xs leading-4 text-danger">
            <span className="font-mono">[{error.code}]</span> {error.message}
          </p>
          {(error.code === 'GEN_BACKEND_NOT_CONFIGURED' || error.code === 'LLM_KEY_REQUIRED') && (
            <button
              type="button"
              data-testid="studio-goto-settings"
              onClick={openModelsSettings}
              className="shrink-0 rounded border border-edge-strong px-1.5 py-0.5 text-[10px] text-fg-2 transition-colors hover:bg-shell-hover"
            >
              去设置
            </button>
          )}
          <button
            type="button"
            onClick={clearError}
            className="shrink-0 text-[10px] text-fg-4 hover:text-fg-2"
          >
            关闭
          </button>
        </div>
      )}
      {run && kind === 'text' && (
        <div
          data-testid="studio-run-status"
          className="mb-1.5 flex flex-wrap items-center gap-2 rounded-lg border border-edge-strong bg-shell-panel px-2.5 py-1.5 shadow-composer"
        >
          <p className="min-w-0 flex-1 text-2xs text-fg-2">
            {run.phase === 'retrieving' && '正在检索项目资产与文档…'}
            {run.phase === 'read' && '已读取资源,继续撰写…'}
            {run.phase === 'pending-approval' && `待审批: ${pending?.tool ?? '写操作'}`}
            {run.phase === 'running' &&
              (run.tools.some((t) => t.status === 'running')
                ? `调用 ${run.tools.find((t) => t.status === 'running')?.name ?? '工具'}…`
                : '生成中…')}
            {run.phase === 'failed' && '本轮失败'}
            {run.tools.length > 0 && (
              <span className="ml-1 text-fg-4">
                {run.tools.filter((t) => t.status === 'ok').length}/{run.tools.length} 工具
              </span>
            )}
          </p>
          {pending && (
            <>
              <button
                type="button"
                data-testid="studio-perm-approve"
                onClick={() => void resolvePermission(true)}
                className="shrink-0 rounded border border-acc px-1.5 py-0.5 text-[10px] text-acc hover:bg-shell-hover"
              >
                批准
              </button>
              <button
                type="button"
                data-testid="studio-perm-deny"
                onClick={() => void resolvePermission(false)}
                className="shrink-0 rounded border border-edge-strong px-1.5 py-0.5 text-[10px] text-fg-2 hover:bg-shell-hover"
              >
                拒绝
              </button>
            </>
          )}
        </div>
      )}
      {/* 清单拉取失败:如实错误 + 重试(不冒充「未配置」——I-5) */}
      {genKind !== null && backendsLoaded && backendsError !== null && error === null && (
        <div
          data-testid="studio-backends-error"
          className="mb-1.5 flex items-center gap-2 rounded-lg border border-danger/50 bg-shell-panel px-2.5 py-1.5 shadow-composer"
        >
          <p className="min-w-0 flex-1 text-2xs leading-4 text-danger">
            生成后端清单拉取失败:{backendsError}
          </p>
          <button
            type="button"
            data-testid="studio-backends-retry"
            onClick={() => void loadBackends()}
            className="shrink-0 rounded border border-edge-strong px-1.5 py-0.5 text-[10px] text-fg-2 transition-colors hover:bg-shell-hover"
          >
            重试
          </button>
        </div>
      )}
      {/* 未配置后端的前置如实提示(不挡发送:发送后拿到同码错误);拉取失败时不显示(上条优先) */}
      {genKind !== null && backendsLoaded && backendsError === null && !anyConfigured && error === null && (
        <div className="mb-1.5 flex items-center gap-2 rounded-lg border border-edge-strong bg-shell-panel px-2.5 py-1.5 shadow-composer">
          <p className="min-w-0 flex-1 text-2xs text-fg-3">
            未配置{preset.label}生成后端(预留 API 端口)——在设置·模型页填入兼容 endpoint / model / key 即可用。
          </p>
          <button
            type="button"
            data-testid="studio-precheck-settings"
            onClick={openModelsSettings}
            className="shrink-0 rounded border border-edge-strong px-1.5 py-0.5 text-[10px] text-fg-2 transition-colors hover:bg-shell-hover"
          >
            去设置
          </button>
        </div>
      )}

      <div className="rounded-xl border border-edge-strong bg-shell-panel shadow-composer">
        {/* text:模板 chips;audio:音频/音乐子 tab */}
        {kind === 'text' && (
          <div className="flex flex-wrap items-center gap-1 border-b border-edge px-2.5 pb-1.5 pt-2">
            {TEXT_TEMPLATES.map((t) => (
              <button
                key={t.id}
                type="button"
                data-testid={`studio-template-${t.id}`}
                title={t.prefix !== '' ? t.prefix : '不加模板前缀,直接发送你的描述'}
                onClick={() => setParam(node.id, 'template', t.id)}
                className={cn(chipBtn, templateId === t.id && chipActive)}
              >
                {t.label}
              </button>
            ))}
            <span className="mx-1 h-3.5 w-px bg-edge-strong" />
            <span className="flex items-center gap-1 text-[10px] text-fg-4">
              <FolderTree size={10} strokeWidth={1.8} />
              资源
            </span>
            <span className={cn(chipBtn, chipActive)} data-testid="studio-scope-current">
              当前项目
            </span>
            {includeLibrary && (
              <span className={cn(chipBtn, chipActive)} data-testid="studio-scope-library">
                素材库
              </span>
            )}
            <div className="relative">
              <button
                type="button"
                data-testid="studio-scope-more"
                onClick={() => setScopeOpen((v) => !v)}
                className={cn(chipBtn, readonlyWorkspaceIds.length > 0 && chipActive)}
              >
                其他项目{readonlyWorkspaceIds.length > 0 ? ` ${readonlyWorkspaceIds.length}` : ''}
              </button>
              {scopeOpen && (
                <div
                  data-testid="studio-scope-picker"
                  className="absolute left-0 top-full z-30 mt-1 w-[220px] rounded-lg border border-edge-strong bg-shell-float p-1 shadow-pop"
                >
                  <p className="px-1.5 py-1 text-[10px] text-fg-4">勾选后可只读检索,不能写入</p>
                  {workspaces
                    .filter((w) => w.id !== activeWorkspaceId)
                    .map((w) => {
                      const on = readonlyWorkspaceIds.includes(w.id);
                      return (
                        <button
                          key={w.id}
                          type="button"
                          data-testid={`studio-scope-ws-${w.id}`}
                          onClick={() =>
                            setReadonlyWorkspaceIds(
                              on
                                ? readonlyWorkspaceIds.filter((x) => x !== w.id)
                                : [...readonlyWorkspaceIds, w.id],
                            )
                          }
                          className="flex w-full items-center gap-1.5 rounded px-1.5 py-1 text-left text-2xs hover:bg-shell-hover"
                        >
                          <span className="min-w-0 flex-1 truncate">{w.name}</span>
                          {on && <Check size={11} className="shrink-0 text-acc" />}
                        </button>
                      );
                    })}
                  {workspaces.filter((w) => w.id !== activeWorkspaceId).length === 0 && (
                    <p className="px-1.5 pb-1 text-2xs text-fg-4">没有其他已登记项目</p>
                  )}
                </div>
              )}
            </div>
          </div>
        )}
        {kind === 'audio' && (
          <div className="flex items-center gap-1 border-b border-edge px-2.5 pb-1.5 pt-2">
            {(['tts', 'music'] as const).map((m) => (
              <button
                key={m}
                type="button"
                data-testid={`studio-audio-mode-${m}`}
                onClick={() => setParam(node.id, 'mode', m)}
                className={cn(
                  'rounded-md border border-edge-strong px-2 py-0.5 text-2xs transition-colors',
                  audioMode === m
                    ? 'bg-shell-active text-fg shadow-sm'
                    : 'text-fg-3 hover:bg-shell-hover hover:text-fg-2',
                )}
              >
                {m === 'tts' ? '音频生成' : '音乐生成'}
              </button>
            ))}
          </div>
        )}

        {/* prompt 主输入 */}
        <div className="relative">
          <textarea
            data-testid="studio-prompt"
            value={node.prompt}
            maxLength={PROMPT_MAX}
            onChange={(e) => setPrompt(node.id, e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) send();
            }}
            placeholder={
              kind === 'audio' && audioMode === 'music' ? '描述你想要的音乐风格…' : preset.hint
            }
            rows={3}
            className="block w-full resize-none bg-transparent px-2.5 py-2 text-xs text-fg outline-none placeholder:text-fg-4"
          />
          <span className="pointer-events-none absolute bottom-1 right-2.5 text-[10px] text-fg-4">
            {node.prompt.length} / {PROMPT_MAX}
          </span>
        </div>

        {/* music:歌词副输入(未填自动纯生成) */}
        {kind === 'audio' && audioMode === 'music' && (
          <div className="border-t border-edge">
            <textarea
              data-testid="studio-lyrics"
              value={typeof node.params.lyrics === 'string' ? node.params.lyrics : ''}
              onChange={(e) => setParam(node.id, 'lyrics', e.target.value)}
              placeholder="请在此添加您的歌词。如果未填歌词,将按曲风自动生成。"
              rows={2}
              className="block w-full resize-none bg-transparent px-2.5 py-1.5 text-2xs text-fg-2 outline-none placeholder:text-fg-4"
            />
          </div>
        )}

        {/* 角色动画:参考图来源 + clip 名(截帧参数在底条) */}
        {kind === 'sprite' && (
          <div
            data-testid="studio-charanim-ref"
            className="flex flex-wrap items-center gap-1.5 border-t border-edge px-2.5 py-1.5"
          >
            <span className="flex items-center gap-1 text-[10px] text-fg-4">
              <ImageIcon size={10} strokeWidth={1.8} />
              参考图
            </span>
            {upstreamRef !== null ? (
              <span
                data-testid="studio-charanim-upstream"
                className={cn(chipBtn, chipActive, 'cursor-default')}
                title="来自上游连线的原画节点当前版本;想改用别的图就断开连线"
              >
                上游「{upstreamRef}」
              </span>
            ) : (
              <select
                data-testid="studio-charanim-refpick"
                value={typeof node.params.refAssetPath === 'string' ? node.params.refAssetPath : ''}
                onChange={(e) => setParam(node.id, 'refAssetPath', e.target.value)}
                className="max-w-[220px] rounded border border-edge-strong bg-shell-sunk px-1 py-px font-mono text-[10px] text-fg outline-none focus:border-fg-4"
              >
                <option value="">(未选:请连上游原画节点或在此选一张贴图)</option>
                {assets
                  .filter((a) => a.type === 'texture')
                  .map((a) => (
                    <option key={a.guid} value={a.path}>
                      {a.path}
                    </option>
                  ))}
              </select>
            )}
            <span className="h-3.5 w-px bg-edge-strong" />
            <label className="flex items-center gap-1 text-[10px] text-fg-4">
              动作名
              <input
                data-testid="studio-charanim-clip"
                value={typeof node.params.clipName === 'string' ? node.params.clipName : ''}
                onChange={(e) => setParam(node.id, 'clipName', e.target.value)}
                placeholder="walk"
                className="w-[80px] rounded border border-edge-strong bg-shell-sunk px-1 py-px font-mono text-[10px] text-fg outline-none placeholder:text-fg-4 focus:border-fg-4"
              />
            </label>
            {ffmpeg !== null && (
              <span
                data-testid="studio-ffmpeg-badge"
                title={
                  ffmpeg.found
                    ? `${ffmpeg.version ?? 'ffmpeg'}\n${ffmpeg.path ?? ''}`
                    : '截帧需要 ffmpeg:装好后置于 PATH,或放到 <workspace>/data/tools/,或用环境变量 FORGE_FFMPEG 指向可执行文件。未装也能生成视频,只是切不出图集。'
                }
                className={cn(
                  'ml-auto rounded border px-1.5 py-px text-[10px]',
                  ffmpeg.found
                    ? 'border-sage/50 text-sage'
                    : 'border-warn/50 text-warn',
                )}
              >
                {ffmpeg.found ? 'ffmpeg 就绪' : 'ffmpeg 未找到 · 只出视频'}
              </span>
            )}
          </div>
        )}

        {/* 底条:模型选择 + 参数 chips + 发送 */}
        <div className="relative flex flex-wrap items-center gap-1.5 border-t border-edge px-2 py-1.5">
          <div className="relative">
            <button
              type="button"
              data-testid="studio-model-btn"
              title={
                kind === 'text'
                  ? 'LLM 渠道由后端自动解析(deepseek/mock);点开查看说明'
                  : '选择生成后端(未配置项生成时如实报错)'
              }
              onClick={() => setPickerOpen((v) => !v)}
              className="flex items-center gap-1 rounded-md px-1.5 py-0.5 text-2xs text-fg-2 transition-colors hover:bg-shell-hover"
            >
              <Cpu size={11} strokeWidth={1.8} className="text-fg-3" />
              <span className="max-w-[150px] truncate font-mono">{modelLabel}</span>
              <ChevronDown size={10} strokeWidth={2} className="text-fg-4" />
            </button>
            {pickerOpen && <ModelPicker node={node} onClose={() => setPickerOpen(false)} />}
          </div>
          <span className="h-3.5 w-px bg-edge-strong" />

          {kind === 'image' && (
            <>
              {[256, 512, 1024].map((s) => (
                <button
                  key={s}
                  type="button"
                  data-testid={`studio-size-${s}`}
                  title={`输出边长 ${s}px(正方形)`}
                  onClick={() => setParam(node.id, 'size', s)}
                  className={cn(chipBtn, node.params.size === s && chipActive)}
                >
                  {s}
                </button>
              ))}
              <span className="h-3.5 w-px bg-edge-strong" />
              {[1, 2, 3, 4].map((n) => (
                <button
                  key={n}
                  type="button"
                  data-testid={`studio-n-${n}`}
                  title={`一次生成 ${n} 张候选`}
                  onClick={() => setParam(node.id, 'n', n)}
                  className={cn(chipBtn, node.params.n === n && chipActive)}
                >
                  ×{n}
                </button>
              ))}
            </>
          )}

          {kind === 'video' && (
            <>
              {['16:9', '9:16', '1:1'].map((a) => (
                <button
                  key={a}
                  type="button"
                  data-testid={`studio-aspect-${a}`}
                  onClick={() => setParam(node.id, 'aspect', a)}
                  className={cn(chipBtn, node.params.aspect === a && chipActive)}
                >
                  {a}
                </button>
              ))}
              <span className="h-3.5 w-px bg-edge-strong" />
              {['720p', '1080p', '2k'].map((r) => (
                <button
                  key={r}
                  type="button"
                  data-testid={`studio-res-${r}`}
                  onClick={() => setParam(node.id, 'resolution', r)}
                  className={cn(chipBtn, node.params.resolution === r && chipActive)}
                >
                  {r}
                </button>
              ))}
              <span className="h-3.5 w-px bg-edge-strong" />
              {[5, 10].map((d) => (
                <button
                  key={d}
                  type="button"
                  data-testid={`studio-dur-${d}`}
                  onClick={() => setParam(node.id, 'durationSec', d)}
                  className={cn(chipBtn, node.params.durationSec === d && chipActive)}
                >
                  {d}s
                </button>
              ))}
            </>
          )}

          {kind === 'sprite' && (
            <>
              {[5, 10].map((d) => (
                <button
                  key={d}
                  type="button"
                  data-testid={`studio-dur-${d}`}
                  title={`视频时长 ${d} 秒`}
                  onClick={() => setParam(node.id, 'durationSec', d)}
                  className={cn(chipBtn, node.params.durationSec === d && chipActive)}
                >
                  {d}s
                </button>
              ))}
              <span className="h-3.5 w-px bg-edge-strong" />
              {[6, 8, 10, 12].map((f) => (
                <button
                  key={f}
                  type="button"
                  data-testid={`studio-fps-${f}`}
                  title={`每秒截 ${f} 帧(帧多更顺,图集也更大)`}
                  onClick={() => setParam(node.id, 'fps', f)}
                  className={cn(chipBtn, node.params.fps === f && chipActive)}
                >
                  {f}fps
                </button>
              ))}
              <span className="h-3.5 w-px bg-edge-strong" />
              {[
                { id: 'auto', label: '自动抠底', tip: '采样四角求底色后抠掉(适合纯色背景的生成片)' },
                { id: 'magenta', label: '品红', tip: '与视口色键同规则,适合刻意用品红做底的片子' },
                { id: 'none', label: '不抠', tip: '原样保留背景' },
              ].map((c) => (
                <button
                  key={c.id}
                  type="button"
                  data-testid={`studio-chroma-${c.id}`}
                  title={c.tip}
                  onClick={() => setParam(node.id, 'chromaKey', c.id)}
                  className={cn(chipBtn, node.params.chromaKey === c.id && chipActive)}
                >
                  {c.label}
                </button>
              ))}
              <span className="h-3.5 w-px bg-edge-strong" />
              {[
                { id: 'union', label: '等大', tip: '所有帧共用一个包围盒:帧尺寸一致,脚底锚不抖' },
                { id: 'tight', label: '紧致', tip: '逐帧贴边裁切:图集更省,但帧尺寸不一' },
              ].map((c) => (
                <button
                  key={c.id}
                  type="button"
                  data-testid={`studio-crop-${c.id}`}
                  title={c.tip}
                  onClick={() => setParam(node.id, 'crop', c.id)}
                  className={cn(chipBtn, node.params.crop === c.id && chipActive)}
                >
                  {c.label}
                </button>
              ))}
            </>
          )}

          {kind === 'audio' && audioMode === 'tts' && (
            <>
              <label className="flex items-center gap-1 text-[10px] text-fg-4">
                音色
                <input
                  data-testid="studio-voice"
                  value={typeof node.params.voice === 'string' ? node.params.voice : ''}
                  onChange={(e) => setParam(node.id, 'voice', e.target.value)}
                  placeholder="alloy"
                  className="w-[72px] rounded border border-edge-strong bg-shell-sunk px-1 py-px font-mono text-[10px] text-fg outline-none placeholder:text-fg-4 focus:border-fg-4"
                />
              </label>
              {['mp3', 'wav'].map((f) => (
                <button
                  key={f}
                  type="button"
                  data-testid={`studio-format-${f}`}
                  onClick={() => setParam(node.id, 'format', f)}
                  className={cn(chipBtn, node.params.format === f && chipActive)}
                >
                  {f}
                </button>
              ))}
            </>
          )}
          {kind === 'audio' && audioMode === 'music' && (
            <button
              type="button"
              data-testid="studio-instrumental"
              title="纯音乐:不带人声"
              onClick={() => setParam(node.id, 'instrumental', node.params.instrumental !== true)}
              className={cn(chipBtn, node.params.instrumental === true && chipActive)}
            >
              纯音乐
            </button>
          )}

          {kind === 'model' && (
            <>
              {[
                { n: 5000, label: '5k面' },
                { n: 30000, label: '30k面' },
                { n: 100000, label: '100k面' },
              ].map((p) => (
                <button
                  key={p.n}
                  type="button"
                  data-testid={`studio-poly-${p.n}`}
                  title={`目标面数约 ${p.n}(再点取消,交给供应商自选)`}
                  onClick={() =>
                    setParam(node.id, 'targetPolycount', node.params.targetPolycount === p.n ? undefined : p.n)
                  }
                  className={cn(chipBtn, node.params.targetPolycount === p.n && chipActive)}
                >
                  {p.label}
                </button>
              ))}
              <span className="h-3.5 w-px bg-edge-strong" />
              <button
                type="button"
                data-testid="studio-mesh-texture"
                title="关掉则只出几何、跳过贴图阶段(更快更省额度)"
                onClick={() => setParam(node.id, 'texture', node.params.texture === false)}
                className={cn(chipBtn, node.params.texture !== false && chipActive)}
              >
                贴图
              </button>
              {node.params.texture !== false &&
                ['2k', '4k'].map((r) => (
                  <button
                    key={r}
                    type="button"
                    data-testid={`studio-texres-${r}`}
                    title={`基础色贴图分辨率 ${r}`}
                    onClick={() => setParam(node.id, 'textureResolution', r)}
                    className={cn(
                      chipBtn,
                      (node.params.textureResolution ?? '2k') === r && chipActive,
                    )}
                  >
                    {r}
                  </button>
                ))}
              <span className="flex items-center gap-1 text-[10px] text-fg-4">
                <Settings2 size={10} strokeWidth={1.8} />
                glb;入库后自动构建 .rxmesh
              </span>
            </>
          )}

          <span className="flex-1" />
          {busy ? (
            <button
              type="button"
              data-testid="studio-cancel"
              title="取消本轮"
              onClick={() => void cancelGenerate(node.id)}
              className="flex h-7 w-7 items-center justify-center rounded-full border border-edge-strong text-fg-2 hover:bg-shell-hover"
            >
              <Square size={11} strokeWidth={2} />
            </button>
          ) : (
            <button
              type="button"
              data-testid="studio-send"
              disabled={node.prompt.trim() === ''}
              title="生成(Ctrl+Enter)"
              onClick={send}
              className="flex h-7 w-7 items-center justify-center rounded-full bg-acc text-fg-inv transition-opacity hover:opacity-90 disabled:opacity-40"
            >
              <ArrowUp size={13} strokeWidth={2} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
