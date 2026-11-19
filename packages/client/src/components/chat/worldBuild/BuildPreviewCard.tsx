import React, { useState, useMemo } from 'react';
import { Box, CheckCircle2, Sliders, RefreshCw, Palette, ExternalLink, AlertCircle, Loader2 } from 'lucide-react';
import { useWorldBuildStore, type WorldBuildBuilding } from '@/lib/worldBuildStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { useSessionStore } from '@/lib/sessionStore';
import type { ChatBlock } from '@/lib/timeline';

type WorldBuildBlockData = Extract<ChatBlock, { kind: 'world_build' }>;

export interface BuildPreviewCardProps {
  block: WorldBuildBlockData;
  disabled?: boolean;
}

const DEFAULT_BUILDINGS: WorldBuildBuilding[] = [
  { id: 'bldg_terminal_01', name: '港口指挥塔 (Terminal Tower)', type: 'command_tower' },
  { id: 'bldg_warehouse_02', name: '重型货仓 (Cargo Warehouse)', type: 'warehouse' },
  { id: 'bldg_dock_03', name: '浮空停泊码头 (Hover Dock)', type: 'dock' },
];

const STYLE_PRESETS = [
  { id: 'cyberpunk_harbor', label: '赛博港口 (Cyberpunk Harbor)' },
  { id: 'industrial_grunge', label: '重工废土 (Industrial Grunge)' },
  { id: 'clean_sci_fi', label: '极简科幻 (Clean Sci-Fi)' },
  { id: 'sunset_dusk', label: '黄昏暖调 (Sunset Dusk)' },
];

export default function BuildPreviewCard({ block, disabled = false }: BuildPreviewCardProps) {
  const [activeTab, setActiveTab] = useState<'tune' | 'rebuild' | 'style'>('tune');
  const [selectedBuildingId, setSelectedBuildingId] = useState<string>('bldg_terminal_01');
  const [tuneInput, setTuneInput] = useState<string>('');
  const [rebuildInput, setRebuildInput] = useState<string>('');
  const [stylePreset, setStylePreset] = useState<string>('cyberpunk_harbor');
  const [localPending, setLocalPending] = useState<boolean>(false);

  const activeSessionId = useSessionStore((st) => st.activeSessionId);
  const openBuildPreview = useWorkbenchStore((st) => st.openBuildPreview);
  const storeState = useWorldBuildStore((st) => st.state);
  const storePending = useWorldBuildStore((st) => st.pending !== null);
  const rebuildBuilding = useWorldBuildStore((st) => st.rebuildBuilding);
  const tuneBuilding = useWorldBuildStore((st) => st.tuneBuilding);
  const adjustStyle = useWorldBuildStore((st) => st.adjustStyle);
  const approvePreview = useWorldBuildStore((st) => st.approvePreview);

  const payload = block.payload || {};
  const isSubmitted = Boolean(block.submitted);
  const isPending = localPending || storePending;
  const isActionDisabled = disabled || isSubmitted || isPending;

  // Extract buildings from block payload or fallback to defaults
  const buildings: WorldBuildBuilding[] = useMemo(() => {
    if (Array.isArray(payload.buildings) && payload.buildings.length > 0) {
      return payload.buildings as WorldBuildBuilding[];
    }
    if (storeState?.buildings && storeState.buildings.length > 0) {
      return storeState.buildings;
    }
    return DEFAULT_BUILDINGS;
  }, [payload.buildings, storeState?.buildings]);

  const verified = payload.verified !== false;
  const rev = block.rev ?? storeState?.previewRev ?? 1;

  const handleOpenWorkbench = () => {
    if (activeSessionId) {
      openBuildPreview(block.slug, activeSessionId, (payload.title as string) || '场景搭建预览');
    }
  };

  const handleTuneSubmit = async () => {
    if (!selectedBuildingId || isActionDisabled) return;
    setLocalPending(true);
    try {
      await tuneBuilding(selectedBuildingId, tuneInput.trim(), undefined, {
        rev,
        id: block.slug,
      });
      setTuneInput('');
    } finally {
      setLocalPending(false);
    }
  };

  const handleRebuildSubmit = async () => {
    if (!selectedBuildingId || isActionDisabled) return;
    setLocalPending(true);
    try {
      await rebuildBuilding(selectedBuildingId, rebuildInput.trim(), {
        rev,
        id: block.slug,
      });
      setRebuildInput('');
    } finally {
      setLocalPending(false);
    }
  };

  const handleStyleSubmit = async () => {
    if (!stylePreset || isActionDisabled) return;
    setLocalPending(true);
    try {
      await adjustStyle(stylePreset, {
        rev,
        id: block.slug,
      });
    } finally {
      setLocalPending(false);
    }
  };

  const handleApproveSubmit = async () => {
    if (isActionDisabled) return;
    setLocalPending(true);
    try {
      await approvePreview({
        rev,
        id: block.slug,
      });
    } finally {
      setLocalPending(false);
    }
  };

  return (
    <div
      data-testid="build-preview-card"
      data-slug={block.slug}
      data-rev={rev}
      data-submitted={isSubmitted ? '1' : undefined}
      className="my-2 flex w-full max-w-2xl flex-col rounded-lg border border-edge bg-shell-panel p-4 shadow-sm text-[13px] text-fg"
    >
      {/* Header */}
      <div className="flex items-center justify-between border-b border-edge/60 pb-3">
        <div className="flex items-center gap-2">
          <Box className="h-4 w-4 text-acc" />
          <span className="font-medium text-fg">
            3D 场景交互预览 (第 {rev} 版)
          </span>
          <span
            data-testid="preview-card-verified"
            className="flex items-center gap-1 rounded bg-emerald-500/10 px-2 py-0.5 text-[11px] font-medium text-emerald-600 dark:text-emerald-400"
          >
            <CheckCircle2 className="h-3 w-3" />
            {verified ? '探针验证通过' : '待验证'}
          </span>
        </div>

        <button
          type="button"
          data-testid="preview-card-open-workbench"
          onClick={handleOpenWorkbench}
          className="flex items-center gap-1 rounded px-2.5 py-1 text-[12px] font-medium text-acc transition-colors hover:bg-acc/10"
        >
          <ExternalLink className="h-3.5 w-3.5" />
          在工作区打开
        </button>
      </div>

      {/* Building selection (shared across tune and rebuild) */}
      {(activeTab === 'tune' || activeTab === 'rebuild') && (
        <div className="mt-3 flex items-center gap-2">
          <label htmlFor="preview-building-select" className="text-[12px] text-fg-3">
            目标建筑:
          </label>
          <select
            id="preview-building-select"
            data-testid="preview-card-building-select"
            value={selectedBuildingId}
            disabled={isActionDisabled}
            onChange={(e) => setSelectedBuildingId(e.target.value)}
            className="flex-1 rounded border border-edge bg-shell-bg px-2.5 py-1 text-[12px] text-fg focus:border-acc focus:outline-none disabled:opacity-50"
          >
            {buildings.map((b) => (
              <option key={b.id} value={b.id}>
                {b.name || b.id} ({b.type})
              </option>
            ))}
          </select>
        </div>
      )}

      {/* Action Tabs */}
      <div className="mt-3 flex border-b border-edge/40 text-[12px]">
        <button
          type="button"
          data-testid="preview-card-tab-tune"
          onClick={() => setActiveTab('tune')}
          className={`flex items-center gap-1.5 border-b-2 px-3 py-1.5 font-medium transition-colors ${
            activeTab === 'tune'
              ? 'border-acc text-acc'
              : 'border-transparent text-fg-3 hover:text-fg'
          }`}
        >
          <Sliders className="h-3.5 w-3.5" />
          微调 (Tune)
        </button>
        <button
          type="button"
          data-testid="preview-card-tab-rebuild"
          onClick={() => setActiveTab('rebuild')}
          className={`flex items-center gap-1.5 border-b-2 px-3 py-1.5 font-medium transition-colors ${
            activeTab === 'rebuild'
              ? 'border-acc text-acc'
              : 'border-transparent text-fg-3 hover:text-fg'
          }`}
        >
          <RefreshCw className="h-3.5 w-3.5" />
          重构 (Rebuild)
        </button>
        <button
          type="button"
          data-testid="preview-card-tab-style"
          onClick={() => setActiveTab('style')}
          className={`flex items-center gap-1.5 border-b-2 px-3 py-1.5 font-medium transition-colors ${
            activeTab === 'style'
              ? 'border-acc text-acc'
              : 'border-transparent text-fg-3 hover:text-fg'
          }`}
        >
          <Palette className="h-3.5 w-3.5" />
          风格 (Style)
        </button>
      </div>

      {/* Tab Panels */}
      <div className="mt-3">
        {activeTab === 'tune' && (
          <div className="flex flex-col gap-2">
            <textarea
              data-testid="preview-card-tune-input"
              value={tuneInput}
              disabled={isActionDisabled}
              placeholder="输入单体建筑微调指令 (例如: 增加雷达天线高度，修改涂装为深灰带橙色条纹)..."
              onChange={(e) => setTuneInput(e.target.value)}
              className="h-16 w-full resize-none rounded border border-edge bg-shell-bg p-2 text-[12px] text-fg focus:border-acc focus:outline-none disabled:opacity-50"
            />
            <div className="flex justify-end">
              <button
                type="button"
                data-testid="preview-card-tune-submit"
                disabled={isActionDisabled || !tuneInput.trim()}
                onClick={handleTuneSubmit}
                className="flex items-center gap-1.5 rounded bg-acc px-3 py-1 text-[12px] font-medium text-white transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
              >
                {isPending && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                提交微调
              </button>
            </div>
          </div>
        )}

        {activeTab === 'rebuild' && (
          <div className="flex flex-col gap-2">
            <textarea
              data-testid="preview-card-rebuild-input"
              value={rebuildInput}
              disabled={isActionDisabled}
              placeholder="输入完全重新生成的建筑提示词 (例如: 改为高科技全息通信塔，带有四组浮动环)..."
              onChange={(e) => setRebuildInput(e.target.value)}
              className="h-16 w-full resize-none rounded border border-edge bg-shell-bg p-2 text-[12px] text-fg focus:border-acc focus:outline-none disabled:opacity-50"
            />
            <div className="flex justify-end">
              <button
                type="button"
                data-testid="preview-card-rebuild-submit"
                disabled={isActionDisabled || !rebuildInput.trim()}
                onClick={handleRebuildSubmit}
                className="flex items-center gap-1.5 rounded bg-acc px-3 py-1 text-[12px] font-medium text-white transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
              >
                {isPending && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                重新生成建筑
              </button>
            </div>
          </div>
        )}

        {activeTab === 'style' && (
          <div className="flex flex-col gap-2">
            <div className="flex items-center gap-2">
              <label htmlFor="preview-style-select" className="text-[12px] text-fg-3">
                风格预设:
              </label>
              <select
                id="preview-style-select"
                data-testid="preview-card-style-select"
                value={stylePreset}
                disabled={isActionDisabled}
                onChange={(e) => setStylePreset(e.target.value)}
                className="flex-1 rounded border border-edge bg-shell-bg px-2.5 py-1 text-[12px] text-fg focus:border-acc focus:outline-none disabled:opacity-50"
              >
                {STYLE_PRESETS.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.label}
                  </option>
                ))}
              </select>
            </div>
            <div className="flex justify-end">
              <button
                type="button"
                data-testid="preview-card-style-submit"
                disabled={isActionDisabled}
                onClick={handleStyleSubmit}
                className="flex items-center gap-1.5 rounded bg-acc px-3 py-1 text-[12px] font-medium text-white transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
              >
                {isPending && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                应用全场风格
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Footer / Approve Action */}
      <div className="mt-4 flex items-center justify-between border-t border-edge/60 pt-3">
        {isSubmitted ? (
          <div
            data-testid="preview-card-submitted"
            className="flex items-center gap-1.5 text-[12px] text-emerald-600 dark:text-emerald-400"
          >
            <CheckCircle2 className="h-3.5 w-3.5" />
            已确认预览并推进至施工阶段
          </div>
        ) : (
          <div className="text-[11px] text-fg-3">
            确认当前 3D 布局与外观满意后，点击右侧批准以进入建筑施工阶段。
          </div>
        )}

        <button
          type="button"
          data-testid="preview-card-approve-submit"
          disabled={isActionDisabled}
          onClick={handleApproveSubmit}
          className="flex items-center gap-1.5 rounded bg-emerald-600 px-3.5 py-1.5 text-[12px] font-semibold text-white shadow-sm transition-opacity hover:bg-emerald-700 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {isPending && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
          批准预览 (Approve)
        </button>
      </div>
    </div>
  );
}
