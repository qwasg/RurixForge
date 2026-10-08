import { useEffect, useState } from 'react';
import { Check } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useToastStore } from '@/lib/toastStore';
import {
  normalizeHex,
  presetById,
  THEME_PRESETS,
  useThemeStore,
  type DiffMarkers,
  type ThemeMode,
  type ThemePalette,
} from '@/lib/themeStore';
import { SetCard, SetH1, SetRow, SetSectionLabel, SetSelect, SetSlider, SetStepper, SetToggle } from './controls';

/**
 * F7 wave.5 外观页(旗舰;参考 ui/appearance_controls.rs appearance_page):
 * 主题模式三卡(系统/浅色/深色,72px 缩略窗,选中 2px accent 边);
 * 预设下拉(THEME_PRESETS 全量 Aa 色板行);「浅色主题」「深色主题」两块各:强调色/背景/前景
 * (18px 圆色板 + 116px hex 输入,非法 toast 不应用)+ 半透明侧边栏 toggle + 对比度
 * slider(0–100 + 读数);UI/代码字体输入;UI 字号 stepper(11–18)/代码字号 stepper(10–20);
 * 差异标记 seg(Color / +/-,D-040 起驱动右栏文件树的 git 改动标记)。全部即改即存即改即预览。
 *
 * 差异留痕:参考预设按侧(light/dark 各自 apply_preset(side));本仓 themeStore.applyPreset
 * 为整块双表替换(wave.3 既有语义),预设选择器做单列全局行,双侧块内不再各挂预设下拉。
 * 参考「预览」diff 卡(theme_diff_preview)依赖参考侧主题包协议,本仓无该数据面,不落。
 */

// ---------- 主题模式三卡(72px 缩略窗;参考 theme_thumbnail) ----------

function MiniWindow({ dark, narrow = false }: { dark: boolean; narrow?: boolean }) {
  const titleBg = dark ? '#232220' : '#EDEBE6';
  const bar = dark ? '#3A382F' : '#D8D4CC';
  const bar2 = dark ? '#2E2C26' : '#E4E1DA';
  const content = dark ? '#1C1B18' : '#FAF9F5';
  const widths = narrow ? [16, 12, 14] : [34, 26, 30];
  return (
    <div className={cn('overflow-hidden rounded-[4px]', narrow ? 'h-full w-full' : 'w-[54px]')}>
      <div
        className="flex h-[10px] w-full flex-col justify-center gap-[1.5px] px-[3px]"
        style={{ background: titleBg }}
      >
        <div className="h-[2px] rounded-[1.5px]" style={{ width: narrow ? 10 : 14, background: bar }} />
        <div className="h-[2px] rounded-[1.5px]" style={{ width: narrow ? 6 : 9, background: bar }} />
      </div>
      <div className="flex flex-col gap-[2px] p-[4px]" style={{ background: content }}>
        <div className="h-[2.5px] rounded-[1.5px]" style={{ width: widths[0], background: bar }} />
        <div className="h-[2.5px] rounded-[1.5px]" style={{ width: widths[1], background: bar2 }} />
        <div className="h-[2.5px] rounded-[1.5px]" style={{ width: widths[2], background: bar }} />
      </div>
    </div>
  );
}

function ModeCard({
  mode,
  label,
  active,
  onPick,
}: {
  mode: ThemeMode;
  label: string;
  active: boolean;
  onPick: (m: ThemeMode) => void;
}) {
  return (
    <button
      type="button"
      data-testid={`theme-mode-${mode}`}
      onClick={() => onPick(mode)}
      className="flex min-w-0 flex-1 flex-col gap-2 text-left"
    >
      <span
        className={cn(
          'flex h-[72px] w-full items-center justify-center overflow-hidden rounded-lg border-2',
          active ? 'border-acc' : 'border-edge',
        )}
        style={{ background: mode === 'dark' ? '#232220' : mode === 'light' ? '#F5F3EE' : undefined }}
      >
        {mode === 'auto' ? (
          <span className="relative flex h-full w-full">
            <span className="flex-1" style={{ background: '#F5F3EE' }} />
            <span className="flex-1" style={{ background: '#232220' }} />
            <span className="absolute inset-0 flex items-center justify-center">
              <span className="flex w-[54px] overflow-hidden rounded-[4px]">
                <span className="min-w-0 flex-1">
                  <MiniWindow dark={false} narrow />
                </span>
                <span className="min-w-0 flex-1">
                  <MiniWindow dark narrow />
                </span>
              </span>
            </span>
          </span>
        ) : (
          <MiniWindow dark={mode === 'dark'} />
        )}
      </span>
      <span className={cn('text-[12px]', active ? 'text-fg' : 'text-fg-3')}>{label}</span>
    </button>
  );
}

// ---------- hex 颜色行(18px 圆色板 + 116px hex 输入,非法 toast 不应用) ----------

function ColorRow({
  label,
  side,
  field,
  last = false,
}: {
  label: string;
  side: 'light' | 'dark';
  field: 'accent' | 'background' | 'foreground';
  last?: boolean;
}) {
  const value = useThemeStore((st) => st[side][field]);
  const setPaletteValue = useThemeStore((st) => st.setPaletteValue);
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);

  const commit = (raw: string) => {
    const normalized = normalizeHex(raw);
    if (normalized === null) {
      useToastStore.getState().push('error', `非法颜色值「${raw}」(须 6 位 hex),未应用`);
      setDraft(value);
      return;
    }
    if (normalized !== value) setPaletteValue(side, field, normalized);
    setDraft(normalized);
  };

  return (
    <SetRow
      title={label}
      last={last}
      testId={`color-row-${side}-${field}`}
      control={
        <span className="flex items-center gap-2">
          <span
            data-testid={`swatch-${side}-${field}`}
            className="h-[18px] w-[18px] shrink-0 rounded-full border border-edge"
            style={{ background: value }}
          />
          <input
            value={draft}
            data-testid={`hex-input-${side}-${field}`}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={(e) => commit(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commit((e.target as HTMLInputElement).value);
            }}
            className="w-[116px] rounded-md border border-edge bg-shell-panel px-2 py-[5px] font-code text-[12px] text-fg outline-none focus:border-edge-strong"
          />
        </span>
      }
    />
  );
}

// ---------- 单侧主题块(浅色/深色) ----------

function ThemeBlock({ side, title }: { side: 'light' | 'dark'; title: string }) {
  const palette: ThemePalette = useThemeStore((st) => st[side]);
  const setPaletteValue = useThemeStore((st) => st.setPaletteValue);
  return (
    <SetCard testId={`theme-block-${side}`}>
      <div className="border-b border-edge px-4 py-3.5 text-[13px] font-semibold text-fg">{title}</div>
      <ColorRow label="强调色" side={side} field="accent" />
      <ColorRow label="背景" side={side} field="background" />
      <ColorRow label="前景" side={side} field="foreground" />
      <SetRow
        title="半透明侧边栏"
        testId={`translucent-row-${side}`}
        control={
          <SetToggle
            on={palette.translucentSidebar}
            testId={`translucent-${side}`}
            onChange={(v) => setPaletteValue(side, 'translucentSidebar', v)}
          />
        }
      />
      <SetRow
        title="对比度"
        last
        testId={`contrast-row-${side}`}
        control={
          <SetSlider
            value={palette.contrast}
            min={0}
            max={100}
            testId={`contrast-${side}`}
            onChange={(v) => setPaletteValue(side, 'contrast', v)}
          />
        }
      />
    </SetCard>
  );
}

// ---------- 预设选择(全量预设 Aa 色板行;参考 theme_preset_select) ----------

function PresetSelect() {
  const presetId = useThemeStore((st) => st.presetId);
  const applyPreset = useThemeStore((st) => st.applyPreset);
  const active = presetById(presetId);
  return (
    <SetSelect
      value={presetId}
      testId="preset-select"
      minWidth={200}
      options={THEME_PRESETS.map((p) => ({ value: p.id, label: p.name }))}
      onPick={(v) => applyPreset(v)}
    />
  );
}

// ---------- 差异标记 seg(Color / +/-) ----------

function DiffMarkerSeg() {
  const diffMarkers = useThemeStore((st) => st.diffMarkers);
  const setDiffMarkers = useThemeStore((st) => st.setDiffMarkers);
  const items: Array<{ id: DiffMarkers; label: string }> = [
    { id: 'color', label: 'Color' },
    { id: 'plusminus', label: '+/-' },
  ];
  return (
    <span className="flex items-center gap-0.5 rounded-md border border-edge bg-shell-sunk p-0.5" data-testid="diff-seg">
      {items.map((it) => (
        <button
          key={it.id}
          type="button"
          data-testid={`diff-seg-${it.id}`}
          onClick={() => setDiffMarkers(it.id)}
          className={cn(
            'flex h-[22px] items-center rounded-[5px] px-2 text-[11.5px]',
            diffMarkers === it.id ? 'bg-shell-panel text-fg shadow-sh1' : 'text-fg-3 hover:text-fg-2',
          )}
        >
          {it.label}
        </button>
      ))}
    </span>
  );
}

export default function AppearancePage() {
  const mode = useThemeStore((st) => st.mode);
  const setMode = useThemeStore((st) => st.setMode);
  const uiFont = useThemeStore((st) => st.uiFont);
  const codeFont = useThemeStore((st) => st.codeFont);
  const uiSize = useThemeStore((st) => st.uiSize);
  const codeSize = useThemeStore((st) => st.codeSize);
  const setUiFont = useThemeStore((st) => st.setUiFont);
  const setCodeFont = useThemeStore((st) => st.setCodeFont);
  const setUiSize = useThemeStore((st) => st.setUiSize);
  const setCodeSize = useThemeStore((st) => st.setCodeSize);
  const presetId = useThemeStore((st) => st.presetId);
  const activePreset = presetById(presetId);

  return (
    <div data-testid="settings-page-appearance" className="flex flex-col">
      <SetH1>外观</SetH1>
      {/* 主题模式三卡 */}
      <SetCard testId="theme-mode-cards">
        <div className="flex gap-3 p-4">
          <ModeCard mode="auto" label="系统" active={mode === 'auto'} onPick={setMode} />
          <ModeCard mode="light" label="浅色" active={mode === 'light'} onPick={setMode} />
          <ModeCard mode="dark" label="深色" active={mode === 'dark'} onPick={setMode} />
        </div>
      </SetCard>

      <SetSectionLabel>预设</SetSectionLabel>
      <SetCard>
        <SetRow
          title="主题预设"
          desc={`整套替换浅色/深色双侧配色(${THEME_PRESETS.length} 个预设)`}
          last
          testId="preset-row"
          control={
            <span className="flex items-center gap-2">
              <span
                data-testid="preset-swatch"
                className="flex h-[22px] w-[22px] items-center justify-center rounded-[5px] text-[10px] font-semibold text-white"
                style={{ background: activePreset?.swatch ?? '#C96442' }}
              >
                Aa
              </span>
              <PresetSelect />
            </span>
          }
        />
      </SetCard>

      <SetSectionLabel>浅色主题</SetSectionLabel>
      <ThemeBlock side="light" title="浅色主题" />
      <SetSectionLabel>深色主题</SetSectionLabel>
      <ThemeBlock side="dark" title="深色主题" />

      <SetSectionLabel>字体与字号</SetSectionLabel>
      <SetCard testId="font-card">
        <SetRow
          title="UI 字体"
          testId="ui-font-row"
          control={
            <input
              value={uiFont}
              placeholder="System UI font stack"
              data-testid="ui-font-input"
              onChange={(e) => setUiFont(e.target.value)}
              className="w-[280px] rounded-md border border-edge bg-shell-panel px-2 py-[5px] text-[12px] text-fg outline-none placeholder:text-fg-4 focus:border-edge-strong"
            />
          }
        />
        <SetRow
          title="代码字体"
          testId="code-font-row"
          control={
            <input
              value={codeFont}
              placeholder="Monospace font stack"
              data-testid="code-font-input"
              onChange={(e) => setCodeFont(e.target.value)}
              className="w-[280px] rounded-md border border-edge bg-shell-panel px-2 py-[5px] text-[12px] text-fg outline-none placeholder:text-fg-4 focus:border-edge-strong"
            />
          }
        />
        <SetRow
          title="UI 字号"
          testId="ui-size-row"
          control={<SetStepper value={uiSize} min={11} max={18} testId="ui-size" onSet={setUiSize} />}
        />
        <SetRow
          title="代码字号"
          last
          testId="code-size-row"
          control={<SetStepper value={codeSize} min={10} max={20} testId="code-size" onSet={setCodeSize} />}
        />
      </SetCard>

      <SetSectionLabel>其他</SetSectionLabel>
      <SetCard>
        <SetRow
          title="差异标记"
          desc="右栏文件树的 git 改动标记:Color = 文件名按状态着色 + 状态字母;+/- = 显示每个文件的增删行数"
          last
          testId="diff-markers-row"
          control={<DiffMarkerSeg />}
        />
      </SetCard>
    </div>
  );
}
