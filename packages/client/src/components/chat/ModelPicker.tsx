import { useEffect, useMemo, useRef, useState } from 'react';
import { Check, ChevronDown, ChevronRight, Search, Sparkles } from 'lucide-react';
import { useChatStore, type SnapshotModel } from '@/lib/chatStore';
import { useEffectiveSpec, type EffectiveSpec } from '@/lib/modelSpec';
import { cn } from '@/lib/cn';

/**
 * Composer 工具行的模型规格选择器(2026-08-25 用户拍板改为 Cursor 式四行主菜单)。
 *
 * chip = 模型 label + 淡色规格后缀(如「deepseek-chat 64K Max」,后缀由 modelSpec 归一);
 * 点开主菜单四行,前三行右侧带当前值 + ChevronRight,悬停/点击向右展开子菜单:
 * - Thinking:行尾开关(不换页)。模型不支持时整行禁用。
 * - Context:窗口档子菜单。固定窗口模型只有一档,如实呈现但点不动。
 * - Effort:reasoning_effort 档子菜单。仅在「思考开 + 该渠道收该字段」时可点,
 *   否则整行禁用并给出 title 说明为什么(deepseek 官方不收 / 思考关着)。
 * - Model:分隔线之下,子菜单带搜索框 + 分组 + needs-key 禁用态,与旧模型菜单同语义。
 *
 * 三档能力面全部来自后端 design-snapshot models[](agentd modelspec.rs),本组件不内置
 * 任何模型知识;四个选择均即时 PATCH 会话落库(chatStore patchSpec:乐观 + 失败回滚)。
 * 菜单整体 bottom-full 上弹,子菜单 left-full 右展——与 Composer 另两个下拉同锚 rootRef。
 */

type SubMenu = 'context' | 'effort' | 'model' | null;

/** onOpen:本菜单展开时通知 Composer 关掉它自己的两个下拉(反向由本组件的 outside-click 承担)。 */
export default function ModelPicker({ onOpen }: { onOpen?: () => void }) {
  const models = useChatStore((st) => st.models);
  const thinkingEnabled = useChatStore((st) => st.thinkingEnabled);
  const pickModel = useChatStore((st) => st.pickModel);
  const setThinking = useChatStore((st) => st.setThinking);
  const pickEffort = useChatStore((st) => st.pickEffort);
  const pickContext = useChatStore((st) => st.pickContext);
  const spec = useEffectiveSpec();

  const [open, setOpen] = useState(false);
  const [sub, setSub] = useState<SubMenu>(null);
  const [query, setQuery] = useState('');
  const rootRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      if (sub) setSub(null);
      else setOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open, sub]);

  // 关菜单时复位子页与搜索词;进模型子菜单时聚焦搜索框。
  useEffect(() => {
    if (!open) {
      setSub(null);
      setQuery('');
    }
  }, [open]);
  useEffect(() => {
    if (sub === 'model') searchRef.current?.focus();
  }, [sub]);

  const closeAll = () => setOpen(false);
  // 可展开行:hover 与 click 都只「确保展开」而非 toggle——否则鼠标移上去已展开、
  // 紧接着的 click 会把它又收回去。收起走 hover 到别的行 / Esc / 关菜单。
  const expand = (key: Exclude<SubMenu, null>, enabled = true) => () => setSub(enabled ? key : null);

  return (
    <div ref={rootRef} className="relative flex items-center">
      <button
        type="button"
        aria-label="选择模型"
        aria-expanded={open}
        data-testid="composer-model"
        onClick={() => {
          if (!open) onOpen?.();
          setOpen((v) => !v);
        }}
        className={cn(
          'flex h-[22px] items-center gap-1 rounded-md px-1.5 text-[11px] hover:bg-shell-hover',
          open ? 'bg-shell-hover text-fg' : 'text-fg-2',
        )}
      >
        <Sparkles size={11} />
        <span className="max-w-[140px] truncate">{spec.modelLabel}</span>
        {spec.suffix !== '' && (
          <span data-testid="composer-model-suffix" className="text-fg-4">
            {spec.suffix}
          </span>
        )}
        <ChevronDown size={10} className="text-fg-3" />
      </button>

      {open && (
        <div
          role="menu"
          data-testid="composer-model-menu"
          className="absolute bottom-full left-0 z-40 mb-1.5 flex min-w-[212px] flex-col rounded-[10px] border border-edge bg-shell-float p-1 shadow-float"
        >
          <SpecRow
            testId="spec-row-thinking"
            label="Thinking"
            disabled={!spec.thinkingSupported}
            disabledHint={`${spec.modelLabel} 不支持思考模式`}
            onHover={() => setSub(null)}
            onSelect={() => void setThinking(!thinkingEnabled)}
            trailing={
              <Switch
                on={spec.thinking}
                disabled={!spec.thinkingSupported}
                testId="spec-thinking-switch"
              />
            }
          />
          <SpecRow
            testId="spec-row-context"
            label="Context"
            value={spec.context?.label}
            disabled={spec.contextOptions.length === 0}
            disabledHint="该模型未声明上下文窗口档"
            expandable
            active={sub === 'context'}
            onHover={expand('context', spec.contextOptions.length > 0)}
            onSelect={expand('context', spec.contextOptions.length > 0)}
          />
          <SpecRow
            testId="spec-row-effort"
            label="Effort"
            value={spec.effort?.label}
            disabled={!spec.effortSupported || !spec.thinking}
            disabledHint={
              spec.effortSupported ? '先开启 Thinking' : `${spec.modelLabel} 不接受推理强度参数`
            }
            expandable
            active={sub === 'effort'}
            onHover={expand('effort', spec.effortSupported && spec.thinking)}
            onSelect={expand('effort', spec.effortSupported && spec.thinking)}
          />
          <div className="my-1 h-px bg-edge" />
          <SpecRow
            testId="spec-row-model"
            label="Model"
            value={spec.modelLabel}
            expandable
            active={sub === 'model'}
            onHover={expand('model')}
            onSelect={expand('model')}
          />

          {sub === 'context' && (
            <SubPanel testId="spec-submenu-context">
              {spec.contextOptions.map((o) => (
                <OptionRow
                  key={o.id}
                  testId={`context-item-${o.id}`}
                  label={o.label}
                  active={o.id === spec.context?.id}
                  onSelect={() => {
                    closeAll();
                    void pickContext(o.id);
                  }}
                />
              ))}
            </SubPanel>
          )}

          {sub === 'effort' && (
            <SubPanel testId="spec-submenu-effort">
              {spec.effortOptions.map((o) => (
                <OptionRow
                  key={o.id}
                  testId={`effort-item-${o.id}`}
                  label={o.label}
                  active={o.id === spec.effort?.id}
                  onSelect={() => {
                    closeAll();
                    void pickEffort(o.id);
                  }}
                />
              ))}
            </SubPanel>
          )}

          {sub === 'model' && (
            <ModelSubMenu
              models={models}
              spec={spec}
              query={query}
              onQuery={setQuery}
              searchRef={searchRef}
              onPick={(id) => {
                closeAll();
                void pickModel(id);
              }}
            />
          )}
        </div>
      )}
    </div>
  );
}

/** 主菜单一行:左标签 + 右当前值(+ 开关或 chevron)。 */
function SpecRow({
  testId,
  label,
  value,
  trailing,
  expandable,
  active,
  disabled,
  disabledHint,
  onHover,
  onSelect,
}: {
  testId: string;
  label: string;
  value?: string;
  trailing?: React.ReactNode;
  expandable?: boolean;
  active?: boolean;
  disabled?: boolean;
  disabledHint?: string;
  onHover: () => void;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      data-testid={testId}
      disabled={disabled}
      title={disabled ? disabledHint : undefined}
      onMouseEnter={onHover}
      onClick={onSelect}
      className={cn(
        'flex h-[26px] items-center gap-2 rounded-md px-2 text-left text-[12px]',
        disabled ? 'cursor-not-allowed opacity-45' : 'hover:bg-shell-selection',
        active && !disabled && 'bg-shell-selection',
      )}
    >
      <span className="min-w-0 flex-1 truncate text-fg-2">{label}</span>
      {value !== undefined && (
        <span data-testid={`${testId}-value`} className="shrink-0 truncate text-[11px] text-fg-4">
          {value}
        </span>
      )}
      {trailing}
      {expandable && <ChevronRight size={11} className="shrink-0 text-fg-3" />}
    </button>
  );
}

/**
 * 子菜单浮层(主菜单右侧展开)。底边与主菜单底边对齐、向上生长:Composer 贴屏幕底,
 * 向下是唯一会被视口切掉的方向,故一律往上长,再由 max-h + 滚动兜住超长模型清单。
 */
function SubPanel({ testId, children }: { testId: string; children: React.ReactNode }) {
  return (
    <div
      role="menu"
      data-testid={testId}
      className="absolute bottom-0 left-full ml-1 flex max-h-[300px] min-w-[152px] flex-col overflow-y-auto rounded-[10px] border border-edge bg-shell-float p-1 shadow-float"
    >
      {children}
    </div>
  );
}

/** 子菜单一行:标签 + 选中 check。 */
function OptionRow({
  testId,
  label,
  detail,
  active,
  disabled,
  disabledHint,
  onSelect,
}: {
  testId: string;
  label: string;
  detail?: string;
  active: boolean;
  disabled?: boolean;
  disabledHint?: string;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      data-testid={testId}
      disabled={disabled}
      title={disabled ? disabledHint : undefined}
      onClick={onSelect}
      className={cn(
        'flex min-h-[26px] items-center gap-2 rounded-md px-2 py-[3px] text-left',
        disabled ? 'cursor-not-allowed opacity-50' : 'hover:bg-shell-selection',
      )}
    >
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-[12px] text-fg-2">{label}</span>
        {detail && <span className="truncate text-[10.5px] text-fg-4">{detail}</span>}
      </span>
      {active && <Check size={11} className="shrink-0 text-acc" />}
    </button>
  );
}

/** 模型子菜单:搜索框 + 按 group 分组 + needs-key 禁用。 */
function ModelSubMenu({
  models,
  spec,
  query,
  onQuery,
  searchRef,
  onPick,
}: {
  models: SnapshotModel[];
  spec: EffectiveSpec;
  query: string;
  onQuery: (v: string) => void;
  searchRef: React.RefObject<HTMLInputElement>;
  onPick: (id: string) => void;
}) {
  const groups = useMemo(() => {
    const q = query.trim().toLowerCase();
    const hit = models.filter(
      (m) =>
        q === '' ||
        m.id.toLowerCase().includes(q) ||
        (m.label ?? '').toLowerCase().includes(q) ||
        (m.provider ?? '').toLowerCase().includes(q),
    );
    const out: Array<{ name: string; items: SnapshotModel[] }> = [];
    for (const m of hit) {
      const name = m.group || m.provider || '模型';
      const bucket = out.find((g) => g.name === name);
      if (bucket) bucket.items.push(m);
      else out.push({ name, items: [m] });
    }
    return out;
  }, [models, query]);

  return (
    <SubPanel testId="spec-submenu-model">
      <div className="flex items-center gap-1.5 px-1.5 pb-1 pt-0.5">
        <Search size={10} className="shrink-0 text-fg-4" />
        <input
          ref={searchRef}
          value={query}
          onChange={(e) => onQuery(e.target.value)}
          placeholder="搜索模型"
          data-testid="model-search"
          className="min-w-0 flex-1 bg-transparent text-[11.5px] text-fg outline-none placeholder:text-fg-4"
        />
      </div>
      {groups.length === 0 && (
        <div className="p-2.5 text-[11.5px] text-fg-4">
          {models.length === 0 ? '暂无可用模型' : '无匹配模型'}
        </div>
      )}
      {groups.map((g) => (
        <div key={g.name} className="flex flex-col">
          <div className="px-2 py-1 text-[9.5px] text-fg-4">{g.name}</div>
          {g.items.map((m) => {
            const needsKey = m.availability === 'needs-key';
            return (
              <OptionRow
                key={m.id}
                testId={`model-item-${m.id}`}
                label={m.label || m.id}
                detail={needsKey ? `${m.provider ?? ''} · 未配置 Key` : m.provider}
                active={m.id === spec.model?.id}
                disabled={needsKey}
                disabledHint="未配置 Key"
                onSelect={() => onPick(m.id)}
              />
            );
          })}
        </div>
      ))}
    </SubPanel>
  );
}

/** Thinking 行尾开关(形态同设置页 SetToggle,尺寸压到菜单行高)。 */
function Switch({ on, disabled, testId }: { on: boolean; disabled?: boolean; testId: string }) {
  return (
    <span
      data-testid={testId}
      data-on={on ? '1' : undefined}
      className={cn(
        'relative h-[14px] w-[26px] shrink-0 rounded-full transition-colors',
        on ? 'bg-sage' : 'bg-shell-active',
        disabled && 'opacity-60',
      )}
    >
      <span
        className={cn(
          'absolute top-[2px] h-[10px] w-[10px] rounded-full bg-shell-float transition-[left]',
          on ? 'left-[14px]' : 'left-[2px]',
        )}
      />
    </span>
  );
}
