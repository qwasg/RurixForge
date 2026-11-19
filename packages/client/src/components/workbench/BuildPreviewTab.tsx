import React, { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import {
  Box,
  Camera,
  CheckCircle2,
  ChevronRight,
  Eye,
  Layers,
  Loader2,
  Palette,
  RefreshCw,
  Search,
  Sliders,
  Sparkles,
} from 'lucide-react';
import {
  useWorldBuildStore,
  previewUrl,
  type WorldBuildBuilding,
  type BuildPreviewSceneState,
} from '@/lib/worldBuildStore';
import { useSessionStore } from '@/lib/sessionStore';

export interface BuildPreviewTabProps {
  slug: string;
  sessionId: string;
  tabId: string;
  title: string;
}

const DEFAULT_BUILDINGS: WorldBuildBuilding[] = [
  {
    id: 'bldg_terminal_01',
    name: '港口指挥塔 (Terminal Tower)',
    type: 'command_tower',
    bounds: { min: [-4, 0, -4], max: [4, 18, 4], dimensions: [8, 18, 8] },
    materials: ['mat_cyber_neon', 'mat_wet_metal'],
    interiorPropsCount: 6,
    status: 'ready',
  },
  {
    id: 'bldg_warehouse_02',
    name: '重型货仓 (Cargo Warehouse)',
    type: 'warehouse',
    bounds: { min: [-6, 0, -5], max: [6, 8, 5], dimensions: [12, 8, 10] },
    materials: ['mat_weathered_iron', 'mat_concrete_pbr'],
    interiorPropsCount: 8,
    status: 'ready',
  },
  {
    id: 'bldg_dock_03',
    name: '浮空停泊码头 (Hover Dock)',
    type: 'dock',
    bounds: { min: [-8, 0, -6], max: [8, 6, 6], dimensions: [16, 6, 12] },
    materials: ['mat_carbon_fiber', 'mat_luminescent_blue'],
    interiorPropsCount: 4,
    status: 'ready',
  },
];

const STYLE_PRESETS = [
  { id: 'cyberpunk_harbor', label: '赛博港口 (Cyberpunk Harbor)' },
  { id: 'industrial_grunge', label: '重工废土 (Industrial Grunge)' },
  { id: 'clean_sci_fi', label: '极简科幻 (Clean Sci-Fi)' },
  { id: 'sunset_dusk', label: '黄昏暖调 (Sunset Dusk)' },
];

export default function BuildPreviewTab({
  slug,
  sessionId,
  title,
}: BuildPreviewTabProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [searchQuery, setSearchQuery] = useState<string>('');
  const [tuneInput, setTuneInput] = useState<string>('');
  const [rebuildInput, setRebuildInput] = useState<string>('');
  const [activeStyle, setActiveStyle] = useState<string>('cyberpunk_harbor');
  const [localPending, setLocalPending] = useState<boolean>(false);
  const [iframeLoaded, setIframeLoaded] = useState<boolean>(false);

  const activeSessionId = useSessionStore((st) => st.activeSessionId);
  const storeState = useWorldBuildStore((st) => st.state);
  const storeScene = useWorldBuildStore((st) => st.sceneState);
  const selectedBuildingId = useWorldBuildStore((st) => st.selectedBuildingId);
  const selectBuilding = useWorldBuildStore((st) => st.selectBuilding);
  const setSceneState = useWorldBuildStore((st) => st.setSceneState);
  const rebuildBuilding = useWorldBuildStore((st) => st.rebuildBuilding);
  const tuneBuilding = useWorldBuildStore((st) => st.tuneBuilding);
  const adjustStyle = useWorldBuildStore((st) => st.adjustStyle);
  const approvePreview = useWorldBuildStore((st) => st.approvePreview);
  const storePending = useWorldBuildStore((st) => st.pending !== null);

  const isCurrentSession = !sessionId || sessionId === activeSessionId;
  const isPending = localPending || storePending;
  const rev = storeState?.previewRev ?? 1;

  // Compute buildings list
  const buildings: WorldBuildBuilding[] = useMemo(() => {
    if (storeScene?.buildings && storeScene.buildings.length > 0) {
      return storeScene.buildings;
    }
    if (storeState?.buildings && storeState.buildings.length > 0) {
      return storeState.buildings;
    }
    return DEFAULT_BUILDINGS;
  }, [storeScene?.buildings, storeState?.buildings]);

  // Filtered buildings
  const filteredBuildings = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return buildings;
    return buildings.filter(
      (b) =>
        b.id.toLowerCase().includes(q) ||
        b.name.toLowerCase().includes(q) ||
        b.type.toLowerCase().includes(q),
    );
  }, [buildings, searchQuery]);

  // Selected building object
  const selectedBuilding = useMemo(() => {
    if (!selectedBuildingId) return null;
    return buildings.find((b) => b.id === selectedBuildingId) ?? null;
  }, [buildings, selectedBuildingId]);

  // Post message to iframe helper
  const postToFrame = useCallback((msg: unknown) => {
    if (!iframeRef.current?.contentWindow) return;
    try {
      iframeRef.current.contentWindow.postMessage(msg, '*');
    } catch {
      // 捕获跨域/断开异常
    }
  }, []);

  // Handle postMessage events from iframe
  useEffect(() => {
    const handleMessage = (e: MessageEvent) => {
      const data = e.data;
      if (!data || typeof data !== 'object') return;

      if (data.type === 'BUILD_PREVIEW_READY') {
        setIframeLoaded(true);
        if (data.sceneState) {
          setSceneState(data.sceneState as BuildPreviewSceneState);
        }
      } else if (data.type === 'BUILD_PREVIEW_SELECTED') {
        selectBuilding(data.buildingId || null);
      } else if (data.type === 'BUILD_PREVIEW_STYLE_CHANGED') {
        if (data.style) {
          setActiveStyle(data.style);
        }
      }
    };

    window.addEventListener('message', handleMessage);
    return () => window.removeEventListener('message', handleMessage);
  }, [selectBuilding, setSceneState]);

  // Sync selected building with iframe
  const handleSelectBuilding = (id: string | null) => {
    selectBuilding(id);
    postToFrame({ type: 'SELECT_BUILDING', buildingId: id });
  };

  // Camera presets
  const handleCameraPreset = (view: 'perspective' | 'top' | 'front' | 'side') => {
    postToFrame({ type: 'SET_CAMERA_VIEW', view });
  };

  // Style change
  const handleStyleChange = async (preset: string) => {
    setActiveStyle(preset);
    postToFrame({ type: 'APPLY_STYLE', style: preset });
    if (isCurrentSession) {
      await adjustStyle(preset, { id: slug, rev });
    }
  };

  // Tune submit
  const handleTuneSubmit = async () => {
    if (!selectedBuildingId || !tuneInput.trim() || isPending || !isCurrentSession) return;
    setLocalPending(true);
    try {
      await tuneBuilding(selectedBuildingId, tuneInput.trim(), undefined, {
        id: slug,
        rev,
      });
      setTuneInput('');
    } finally {
      setLocalPending(false);
    }
  };

  // Rebuild submit
  const handleRebuildSubmit = async () => {
    if (!selectedBuildingId || !rebuildInput.trim() || isPending || !isCurrentSession) return;
    setLocalPending(true);
    try {
      await rebuildBuilding(selectedBuildingId, rebuildInput.trim(), {
        id: slug,
        rev,
      });
      setRebuildInput('');
    } finally {
      setLocalPending(false);
    }
  };

  // Approve submit
  const handleApproveSubmit = async () => {
    if (isPending || !isCurrentSession) return;
    setLocalPending(true);
    try {
      await approvePreview({ id: slug, rev });
    } finally {
      setLocalPending(false);
    }
  };

  const iframeSrc = previewUrl(
    slug,
    storeState?.token,
    undefined,
    rev,
    typeof location === 'undefined' ? '' : location.hostname,
  );

  return (
    <div
      data-testid="build-preview-tab"
      className="flex h-full w-full flex-col overflow-hidden bg-shell-bg text-fg select-none"
    >
      {/* Top Controls Toolbar */}
      <div className="flex h-11 items-center justify-between border-b border-edge bg-shell-panel px-3">
        {/* Left: View Title & Indicator */}
        <div className="flex items-center gap-2">
          <Box className="h-4 w-4 text-acc" />
          <span className="font-semibold text-[13px]">{title || '3D 场景预览'}</span>
          <span className="rounded bg-acc/10 px-1.5 py-0.5 text-[11px] font-medium text-acc">
            v{rev}
          </span>
          {!isCurrentSession && (
            <span className="rounded bg-warn/15 px-2 py-0.5 text-[11px] text-warn">
              只读 (属于其他会话)
            </span>
          )}
        </div>

        {/* Center: Camera Presets */}
        <div className="flex items-center gap-1 rounded-md border border-edge/60 bg-shell-bg/70 p-0.5 text-[12px]">
          <span className="px-1.5 text-[11px] text-fg-3 flex items-center gap-1">
            <Camera className="h-3 w-3" />
            视角:
          </span>
          <button
            type="button"
            data-testid="preview-tab-cam-perspective"
            onClick={() => handleCameraPreset('perspective')}
            className="rounded px-2 py-1 text-fg hover:bg-shell-hover active:bg-shell-hover/80"
          >
            透视
          </button>
          <button
            type="button"
            data-testid="preview-tab-cam-top"
            onClick={() => handleCameraPreset('top')}
            className="rounded px-2 py-1 text-fg hover:bg-shell-hover active:bg-shell-hover/80"
          >
            顶视
          </button>
          <button
            type="button"
            data-testid="preview-tab-cam-front"
            onClick={() => handleCameraPreset('front')}
            className="rounded px-2 py-1 text-fg hover:bg-shell-hover active:bg-shell-hover/80"
          >
            正视
          </button>
          <button
            type="button"
            data-testid="preview-tab-cam-side"
            onClick={() => handleCameraPreset('side')}
            className="rounded px-2 py-1 text-fg hover:bg-shell-hover active:bg-shell-hover/80"
          >
            侧视
          </button>
        </div>

        {/* Right: Style Presets & Approve */}
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-1.5 text-[12px]">
            <Palette className="h-3.5 w-3.5 text-fg-3" />
            <select
              data-testid="preview-tab-style-dropdown"
              value={activeStyle}
              disabled={!isCurrentSession || isPending}
              onChange={(e) => handleStyleChange(e.target.value)}
              className="rounded border border-edge bg-shell-bg px-2 py-1 text-[12px] text-fg focus:border-acc focus:outline-none disabled:opacity-50"
            >
              {STYLE_PRESETS.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </select>
          </div>

          <button
            type="button"
            data-testid="preview-tab-approve-btn"
            disabled={!isCurrentSession || isPending}
            onClick={handleApproveSubmit}
            className="flex items-center gap-1.5 rounded bg-emerald-600 px-3 py-1 text-[12px] font-medium text-white transition-opacity hover:bg-emerald-700 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {isPending ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <CheckCircle2 className="h-3.5 w-3.5" />
            )}
            批准预览并施工
          </button>
        </div>
      </div>

      {/* Main Area: Sidebar + 3D Viewport */}
      <div className="flex flex-1 min-h-0 min-w-0">
        {/* Left Sidebar: Buildings hierarchy & Inspector */}
        <div className="flex w-72 flex-col border-r border-edge bg-shell-panel/50">
          {/* Search Header */}
          <div className="border-b border-edge/60 p-2.5">
            <div className="relative flex items-center">
              <Search className="absolute left-2.5 h-3.5 w-3.5 text-fg-3" />
              <input
                type="text"
                data-testid="preview-building-search"
                value={searchQuery}
                placeholder="搜索场景建筑..."
                onChange={(e) => setSearchQuery(e.target.value)}
                className="w-full rounded border border-edge bg-shell-bg py-1 pl-8 pr-2.5 text-[12px] text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
              />
            </div>
          </div>

          {/* Building Hierarchy List */}
          <div className="flex-1 overflow-y-auto p-1.5">
            <div className="px-2 py-1 text-[11px] font-semibold text-fg-3 uppercase tracking-wider">
              建筑清单 ({filteredBuildings.length})
            </div>
            <div className="flex flex-col gap-0.5">
              {filteredBuildings.map((b) => {
                const isSelected = selectedBuildingId === b.id;
                return (
                  <button
                    key={b.id}
                    type="button"
                    data-testid={`preview-bldg-item-${b.id}`}
                    onClick={() => handleSelectBuilding(isSelected ? null : b.id)}
                    className={`flex items-center justify-between rounded px-2.5 py-1.5 text-left text-[12px] transition-colors ${
                      isSelected
                        ? 'bg-acc/15 text-acc font-medium'
                        : 'text-fg hover:bg-shell-hover'
                    }`}
                  >
                    <div className="flex items-center gap-2 truncate">
                      <Layers className="h-3.5 w-3.5 shrink-0 opacity-70" />
                      <span className="truncate">{b.name || b.id}</span>
                    </div>
                    <span className="text-[10px] text-fg-3 shrink-0 ml-1">
                      {b.type}
                    </span>
                  </button>
                );
              })}
              {filteredBuildings.length === 0 && (
                <div className="p-4 text-center text-[12px] text-fg-3">
                  未匹配到相关建筑
                </div>
              )}
            </div>
          </div>

          {/* Inspector Panel for Selected Building */}
          {selectedBuilding && (
            <div
              data-testid="preview-selected-inspector"
              className="border-t border-edge bg-shell-panel p-3 text-[12px]"
            >
              <div className="flex items-center justify-between pb-2 border-b border-edge/60">
                <span className="font-semibold text-fg truncate">
                  {selectedBuilding.name}
                </span>
                <span className="text-[11px] text-fg-3">
                  {selectedBuilding.id}
                </span>
              </div>

              {/* Dimensions & Properties */}
              <div className="mt-2 text-[11px] text-fg-3">
                <div>类型: {selectedBuilding.type}</div>
                {selectedBuilding.bounds?.dimensions && (
                  <div>
                    尺寸: {selectedBuilding.bounds.dimensions.join(' × ')}m
                  </div>
                )}
                {selectedBuilding.materials && (
                  <div>材质数: {selectedBuilding.materials.length}</div>
                )}
              </div>

              {/* Quick Actions (Tune & Rebuild) */}
              <div className="mt-3 flex flex-col gap-2">
                <div className="flex flex-col gap-1">
                  <label className="text-[11px] text-fg-3 flex items-center gap-1">
                    <Sliders className="h-3 w-3" />
                    微调指令:
                  </label>
                  <input
                    type="text"
                    data-testid="preview-tab-tune-input"
                    value={tuneInput}
                    disabled={!isCurrentSession || isPending}
                    placeholder="如: 加宽走廊，修改照明..."
                    onChange={(e) => setTuneInput(e.target.value)}
                    className="w-full rounded border border-edge bg-shell-bg px-2 py-1 text-[11px] text-fg focus:border-acc focus:outline-none disabled:opacity-50"
                  />
                  <button
                    type="button"
                    data-testid="preview-tab-tune-submit"
                    disabled={!isCurrentSession || isPending || !tuneInput.trim()}
                    onClick={handleTuneSubmit}
                    className="rounded bg-acc/10 px-2 py-1 text-[11px] font-medium text-acc hover:bg-acc/20 disabled:opacity-40 disabled:cursor-not-allowed"
                  >
                    提交微调
                  </button>
                </div>

                <div className="flex flex-col gap-1 border-t border-edge/40 pt-2">
                  <label className="text-[11px] text-fg-3 flex items-center gap-1">
                    <RefreshCw className="h-3 w-3" />
                    完全重构:
                  </label>
                  <input
                    type="text"
                    data-testid="preview-tab-rebuild-input"
                    value={rebuildInput}
                    disabled={!isCurrentSession || isPending}
                    placeholder="如: 改为螺旋形塔楼..."
                    onChange={(e) => setRebuildInput(e.target.value)}
                    className="w-full rounded border border-edge bg-shell-bg px-2 py-1 text-[11px] text-fg focus:border-acc focus:outline-none disabled:opacity-50"
                  />
                  <button
                    type="button"
                    data-testid="preview-tab-rebuild-submit"
                    disabled={!isCurrentSession || isPending || !rebuildInput.trim()}
                    onClick={handleRebuildSubmit}
                    className="rounded bg-acc/10 px-2 py-1 text-[11px] font-medium text-acc hover:bg-acc/20 disabled:opacity-40 disabled:cursor-not-allowed"
                  >
                    重构单体
                  </button>
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Center: Sandboxed 3D Viewport iframe */}
        <div className="relative flex-1 bg-black flex items-center justify-center">
          <iframe
            ref={iframeRef}
            data-testid="preview-tab-frame"
            src={iframeSrc}
            sandbox="allow-scripts allow-same-origin allow-pointer-lock"
            className="h-full w-full border-none"
            title="3D World Build Preview"
          />
        </div>
      </div>
    </div>
  );
}
