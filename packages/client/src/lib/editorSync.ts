import { useAssetStore } from './assetStore';
import { useEditorStore } from './editorStore';
import { bareName } from './timeline';
import { readActiveWorkspaceId } from './activeWorkspace';
import { useGraphStore } from './graphStore';

/** A restarted project bus can reuse low sequence numbers; epoch is the boundary. */
export function editorEventCursor() {
  let epoch: string | undefined;
  let seq = -1;
  return (event: { epoch?: string; seq?: number; type?: string }): 'reset' | 'change' | 'ignore' => {
    const restarted = event.epoch !== undefined && event.epoch !== epoch;
    if (restarted) { epoch = event.epoch; seq = -1; }
    if (event.type === 'editor.reset' || restarted) { seq = event.seq ?? seq; return 'reset'; }
    if (event.seq !== undefined && event.seq <= seq) return 'ignore';
    seq = event.seq ?? seq;
    return 'change';
  };
}

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
  'prefab_instantiate', 'model_instantiate', 'sprite_instantiate', 'character_create', 'character_update', 'material_bind', 'shader_graph_bind', 'editor_apply',
]);

const PLAY_TOOLS = new Set(['play_enter', 'play_exit', 'play_pause', 'play_resume', 'play_step']);

const ASSET_MUTATING = new Set([
  'asset_import',
  'asset_delete',
  'asset_move',
  'texture_process',
  'material_create',
  'gen_accept',
  'asset_set_meta', 'sprite_create', 'shader_graph_save', 'shader_graph_compile', 'material_update',
]);

let timer: ReturnType<typeof setTimeout> | null = null;
let pending = { scene: false, play: false, asset: false, graph: false, workspaceId: readActiveWorkspaceId() };

export function notifyAgentToolSettled(toolName: string, workspaceId = readActiveWorkspaceId()): void {
  if (workspaceId !== readActiveWorkspaceId()) return;
  const bare = bareName(toolName);
  const scene = SCENE_MUTATING.has(bare);
  const play = PLAY_TOOLS.has(bare);
  const asset = ASSET_MUTATING.has(bare);
  const graph = bare === 'graph_create' || bare === 'graph_update';
  if (!scene && !play && !asset && !graph) return;
  if (pending.workspaceId !== workspaceId) pending = { scene: false, play: false, asset: false, graph: false, workspaceId };
  pending = { scene: pending.scene || scene, play: pending.play || play, asset: pending.asset || asset, graph: pending.graph || graph, workspaceId };
  if (timer !== null) clearTimeout(timer);
  timer = setTimeout(() => {
    timer = null;
    const { scene, play, asset, graph, workspaceId } = pending;
    pending = { scene: false, play: false, asset: false, graph: false, workspaceId: readActiveWorkspaceId() };
    if (workspaceId !== readActiveWorkspaceId()) return;
    if (scene || play) {
      const st = useEditorStore.getState();
      if (scene) {
        void st.loadEntities();
        void st.refreshSummary();
      }
      if (play) void st.refreshPlayState();
    }
    if (asset) void useAssetStore.getState().load();
    const gs = useGraphStore.getState();
    if (graph && gs.graphPath && !gs.dirty) void gs.loadByPath(gs.graphPath);
  }, 300);
}
