import { useCallback, useEffect, useState } from 'react';
import { Save } from 'lucide-react';
import { useAssetStore, type AssetSemantic } from '@/lib/assetStore';
import { useToastStore } from '@/lib/toastStore';
import Thumb from './assetThumb';

/**
 * F10 资产检视器(右栏第四页签「资产」):选中资产的文字简介/标签编辑
 * (草稿 + 失焦/Ctrl+Enter 提交,照 EntityInspectorPanel 范式)+ 溯源如实显示
 * (source=human|agent-vision|agent-facts / model / updated_at,I-7)。
 * 写路径 = asset_set_description(source=human);写后 asset_list 刷新 + semantic 重取。
 */

const SOURCE_LABEL: Record<string, string> = {
  human: '人工编写',
  'agent-vision': 'Agent(看图)',
  'agent-facts': 'Agent(凭事实)',
};

export default function AssetInspectorPanel() {
  const items = useAssetStore((s) => s.items);
  const selectedGuid = useAssetStore((s) => s.selectedGuid);
  const status = useAssetStore((s) => s.status);
  const fetchSemantic = useAssetStore((s) => s.fetchSemantic);
  const setDescription = useAssetStore((s) => s.setDescription);
  const loadAssets = useAssetStore((s) => s.load);

  const item = items.find((i) => i.guid === selectedGuid) ?? null;

  const [descDraft, setDescDraft] = useState('');
  const [tagsDraft, setTagsDraft] = useState('');
  const [semantic, setSemantic] = useState<AssetSemantic | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (items.length === 0) void loadAssets();
  }, [items.length, loadAssets]);

  // 选中变化:草稿回填 + semantic 溯源重取。
  useEffect(() => {
    setDescDraft(item?.description ?? '');
    setTagsDraft((item?.tags ?? []).join(', '));
    setSemantic(null);
    if (item) {
      void fetchSemantic(item.path).then(setSemantic);
    }
  }, [item?.guid, item?.description, item?.tags, item?.path, fetchSemantic, item]);

  const dirty =
    item !== null &&
    (descDraft.trim() !== (item.description ?? '').trim() ||
      tagsDraft
        .split(/[,，、\s]+/)
        .filter(Boolean)
        .join(',') !== (item.tags ?? []).join(','));

  const save = useCallback(async () => {
    if (!item || saving) return;
    const desc = descDraft.trim();
    if (desc === '') {
      useToastStore.getState().push('error', '简介不可为空');
      return;
    }
    const tags = tagsDraft.split(/[,，、\s]+/).filter(Boolean);
    setSaving(true);
    try {
      await setDescription(item.path, desc, tags);
      const sem = await fetchSemantic(item.path);
      setSemantic(sem);
      useToastStore.getState().push('success', '简介已保存(source=human)');
    } catch (err) {
      useToastStore.getState().push('error', `保存失败:${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setSaving(false);
    }
  }, [item, saving, descDraft, tagsDraft, setDescription, fetchSemantic]);

  if (!item) {
    return (
      <div className="flex min-h-0 flex-1 flex-col bg-shell-sidebar" data-testid="asset-inspector">
        <p className="px-3 pt-2 text-xs text-fg-4">在资产面板点击选中素材,查看并编辑文字简介</p>
      </div>
    );
  }

  const buildState = status[item.path];

  return (
    <div
      className="flex min-h-0 flex-1 flex-col bg-shell-sidebar"
      data-testid="asset-inspector"
      data-asset-guid={item.guid}
    >
      <div className="min-h-0 flex-1 overflow-y-auto pb-2">
        {/* 头部:缩略图 + 基础信息 */}
        <div className="space-y-1.5 px-2.5 pb-2 pt-1">
          <div className="flex items-center gap-2">
            <div className="h-12 w-12 shrink-0">
              <Thumb item={item} size={22} />
            </div>
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm text-fg" title={item.path}>
                {item.path.split('/').pop()}
              </p>
              <p className="truncate font-mono text-2xs text-fg-4" title={item.guid}>
                {item.guid || '(无 GUID)'}
              </p>
            </div>
          </div>
          <div className="flex flex-wrap gap-x-3 gap-y-0.5 text-2xs text-fg-3">
            <span>类型:{item.type}</span>
            <span>大小:{item.size}B</span>
            {buildState && <span>构建:{buildState}</span>}
          </div>
        </div>

        {/* 简介编辑 */}
        <div className="border-t border-edge px-2.5 py-2">
          <p className="pb-1 text-2xs font-medium text-fg-2">文字简介</p>
          <textarea
            value={descDraft}
            onChange={(e) => setDescDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) void save();
              if (e.key === 'Escape') setDescDraft(item.description ?? '');
            }}
            placeholder="一两句说清:是什么/什么风格/适合什么场合(进 RAG 检索与上下文注入)"
            rows={4}
            data-testid="asset-desc-input"
            className="w-full resize-y rounded-md border border-edge bg-shell-input px-2 py-1 text-xs text-fg outline-none focus:border-fg-4"
          />
          <p className="pb-1 pt-1.5 text-2xs font-medium text-fg-2">标签</p>
          <input
            value={tagsDraft}
            onChange={(e) => setTagsDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') void save();
            }}
            placeholder="逗号/空格分隔,如:家具, 椅子, 木质"
            data-testid="asset-tags-input"
            className="w-full rounded-md border border-edge bg-shell-input px-2 py-1 text-xs text-fg outline-none focus:border-fg-4"
          />
          <div className="flex items-center gap-2 pt-1.5">
            <button
              type="button"
              disabled={!dirty || saving || descDraft.trim() === ''}
              onClick={() => void save()}
              data-testid="asset-desc-save"
              className="flex items-center gap-1 rounded-md bg-acc px-2 py-1 text-2xs text-white transition-opacity disabled:opacity-40"
            >
              <Save size={11} />
              {saving ? '保存中…' : '保存(Ctrl+Enter)'}
            </button>
            {dirty && <span className="text-2xs text-warn">未保存</span>}
          </div>
        </div>

        {/* 溯源(I-7 如实显示;无 semantic = 尚无描述) */}
        <div className="border-t border-edge px-2.5 py-2">
          <p className="pb-1 text-2xs font-medium text-fg-2">溯源</p>
          {semantic && (semantic.description ?? '') !== '' ? (
            <div className="space-y-0.5 text-2xs text-fg-3" data-testid="asset-desc-provenance">
              <p>来源:{SOURCE_LABEL[semantic.source ?? ''] ?? semantic.source ?? '未知'}</p>
              {semantic.model && <p>模型:{semantic.model}</p>}
              {semantic.updated_at && <p>更新:{semantic.updated_at}</p>}
            </div>
          ) : (
            <p className="text-2xs text-fg-4">尚无描述记录(保存后此处显示 source/model/时间)</p>
          )}
        </div>

        {/* 标签 chips 预览 */}
        {(item.tags ?? []).length > 0 && (
          <div className="border-t border-edge px-2.5 py-2">
            <p className="pb-1 text-2xs font-medium text-fg-2">已存标签</p>
            <div className="flex flex-wrap gap-1">
              {(item.tags ?? []).map((t) => (
                <span key={t} className="rounded-full bg-shell-active px-2 py-px text-2xs text-fg-2">
                  {t}
                </span>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
