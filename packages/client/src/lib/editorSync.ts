import { useAssetStore } from './assetStore';
import { useEditorStore } from './editorStore';
import { bareName } from './timeline';

/**
 * UI 融合波 C3:agent MCP 工具事件 → 编辑器/资产面板精准刷新。
 * chatStore applyEvent 在 agent.tool.completed|failed 时回调本模块(invoked 侧记
 * toolCallId→name 映射,completed 载荷无 name,F7 事件面如实)。
 * - engine-scene 变更类白名单命中 → 300ms 拖尾防抖重拉(实体清单/场景摘要);
 * - play.* → 附刷 playState;
 * - asset-pipeline/gen 变更类 → 重拉资产清单;
 * 只读工具不触发。EditorView 1s 轮询腿保留(覆盖外部 MCP 客户端直连场景)。
 */

const SCENE_MUTATING = new Set([
  'entity_create',
  'entity_destroy',
  'entity_rename',
  'entity_batch_apply',
  'transform_set',
  'component_add',
  'component_remove',
  'component_set',
  'scene_load',
  'scene_new',
  'edit_undo',
  'edit_redo',
]);

const PLAY_TOOLS = new Set(['play_enter', 'play_exit', 'play_pause', 'play_resume', 'play_step']);

const ASSET_MUTATING = new Set([
  'asset_import',
  'asset_delete',
  'asset_move',
  'texture_process',
  'material_create',
  'gen_accept',
]);

let timer: ReturnType<typeof setTimeout> | null = null;

export function notifyAgentToolSettled(toolName: string): void {
  const bare = bareName(toolName);
  const scene = SCENE_MUTATING.has(bare);
  const play = PLAY_TOOLS.has(bare);
  const asset = ASSET_MUTATING.has(bare);
  if (!scene && !play && !asset) return;
  if (timer !== null) clearTimeout(timer);
  timer = setTimeout(() => {
    timer = null;
    if (scene || play) {
      const st = useEditorStore.getState();
      if (scene) {
        void st.loadEntities();
        void st.refreshSummary();
      }
      if (play) void st.refreshPlayState();
    }
    if (asset) void useAssetStore.getState().load();
  }, 300);
}
