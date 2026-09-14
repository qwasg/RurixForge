import { useEditorStore } from './editorStore';
import { useWorkbenchStore } from './workbenchStore';

/**
 * UI 融合波 C2:消息内实体引用回跳(agent-IDE → 游戏编辑器方向)。
 * 点击消息中的 #id → 打开编辑器 tab → 选中该实体 → 相机聚焦(F 语义,07 §2)。
 * id 不在当前场景实体清单时不动作(诚实:不伪造跳转,不盲选)。
 */
export function jumpToEntity(id: number): void {
  const st = useEditorStore.getState();
  if (!st.entities.some((e) => e.id === id)) return;
  useWorkbenchStore.getState().openEditor();
  st.selectEntity(id);
  void st.focusSelected();
}
