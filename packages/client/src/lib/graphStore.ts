import { create } from 'zustand';
import { callCodeTool } from './forgeApi';
import { useEditorStore } from './editorStore';

/**
 * F4 wave.4 NodeGraph 面板数据面(07 §1 G 区 / 10 §5)。
 * 定位 = 审阅/微调/改常量:查看 graph_get 真实图、内联改 const/暴露属性默认值、
 * 保存即全图校验(graph_validate 不过不落盘)。从零生成/重连走 Chat agent
 * (logic-blueprint-gen),本面板不做拖新节点/拖线连接。
 */

/** 暴露属性种类(与 forge-logic PropKind 对齐) */
export type PropKind = 'F32' | 'I32' | 'Bool' | 'String' | 'Vec3';
/** const 字面量值(JSON 值子集) */
export type GraphValue = string | number | boolean | number[] | Record<string, unknown>;

export interface ExposedProp {
  name: string;
  kind: PropKind;
  default: GraphValue;
}

/** 值来源三态(10 §4.1,untagged 依键判别):const 常量 / node+pin 数据边 / ref 暴露属性 */
export type ValueSource =
  | { const: GraphValue }
  | { node: string; pin: string }
  | { ref: string };

export interface GraphNode {
  id: string;
  type: string;
  pos: [number, number];
  inputs?: Record<string, ValueSource>;
}

/** 执行边:from = [节点 id, 执行出口 pin],to = [节点 id, "exec"] */
export interface GraphEdge {
  from: [string, string];
  to: [string, string];
}

/** .rxgraph 文档(与 forge-logic GraphDoc serde 对齐) */
export interface GraphDoc {
  version: number;
  id: string;
  name: string;
  exposedProps: ExposedProp[];
  nodes: GraphNode[];
  edges: GraphEdge[];
}

/** 校验错误(graph_validate 返回;nodeId 可空 = 图级错误) */
export interface GraphError {
  code: string;
  message: string;
  nodeId?: string;
}

interface GraphState {
  graph: GraphDoc | null;
  graphPath: string | null;
  errors: GraphError[];
  dirty: boolean;
  loading: boolean;
  lastError: string | null;
  /** 最近一次保存成功落盘路径(状态行显示用) */
  lastSaved: string | null;

  loadByPath: (path: string) => Promise<void>;
  loadForSelectedEntity: () => Promise<void>;
  editConst: (nodeId: string, pinName: string, value: GraphValue) => void;
  editExposedDefault: (name: string, value: GraphValue) => void;
  save: () => Promise<void>;
}

/** graph_create 的 name:graphPath basename(去 .rxgraph),退化图 id */
function graphNameForSave(graph: GraphDoc, graphPath: string | null): string {
  if (graphPath) {
    const base = graphPath.split('/').pop() ?? '';
    const stem = base.replace(/\.rxgraph$/i, '');
    if (stem !== '') return stem;
  }
  return graph.id;
}

export const useGraphStore = create<GraphState>((set, get) => ({
  graph: null,
  graphPath: null,
  errors: [],
  dirty: false,
  loading: false,
  lastError: null,
  lastSaved: null,

  loadByPath: async (path) => {
    const p = path.trim();
    if (p === '') return;
    set({ loading: true, lastError: null, lastSaved: null });
    try {
      const r = await callCodeTool<{ graph: GraphDoc }>('graph_get', { path: p });
      set({
        graph: r.graph,
        graphPath: p,
        errors: [],
        dirty: false,
        loading: false,
        lastError: null,
      });
    } catch (err) {
      // 失败如实:lastError 进状态行,不伪造成空图
      set({ lastError: (err as Error).message, loading: false });
    }
  },

  loadForSelectedEntity: async () => {
    const { entities, selectedId } = useEditorStore.getState();
    const entity = entities.find((e) => e.id === selectedId);
    const script = entity?.components.find((c) => c.type === 'Script');
    const ref = script?.props?.graphRef;
    if (typeof ref === 'string' && ref.trim() !== '') {
      await get().loadByPath(ref);
    } else {
      // 无 Script.graphRef:空态(路径输入框仍可手动加载)
      set({
        graph: null,
        graphPath: null,
        errors: [],
        dirty: false,
        lastError: null,
        lastSaved: null,
      });
    }
  },

  editConst: (nodeId, pinName, value) =>
    set((s) => {
      if (!s.graph) return s;
      const nodes = s.graph.nodes.map((n) =>
        n.id === nodeId
          ? { ...n, inputs: { ...(n.inputs ?? {}), [pinName]: { const: value } } }
          : n,
      );
      return { graph: { ...s.graph, nodes }, dirty: true, lastSaved: null };
    }),

  editExposedDefault: (name, value) =>
    set((s) => {
      if (!s.graph) return s;
      const exposedProps = s.graph.exposedProps.map((p) =>
        p.name === name ? { ...p, default: value } : p,
      );
      return { graph: { ...s.graph, exposedProps }, dirty: true, lastSaved: null };
    }),

  save: async () => {
    const { graph, graphPath } = get();
    if (!graph || get().loading) return;
    set({ loading: true, lastError: null, lastSaved: null });
    try {
      // 保存即全图校验(10 §5):不过 → errors 展示、不落盘
      const v = await callCodeTool<{ ok: boolean; errors?: GraphError[] }>('graph_validate', {
        graph,
      });
      if (!v.ok) {
        set({ errors: v.errors ?? [], loading: false });
        return;
      }
      const name = graphNameForSave(graph, graphPath);
      const c = await callCodeTool<{ ok: boolean; path?: string; errors?: GraphError[] }>(
        'graph_create',
        { name, graph },
      );
      if (!c.ok) {
        // create 内复核校验(与 validate 同错误面),同样不落盘
        set({ errors: c.errors ?? [], loading: false });
        return;
      }
      set({
        errors: [],
        dirty: false,
        loading: false,
        graphPath: c.path ?? graphPath,
        lastSaved: c.path ?? graphPath ?? name,
      });
    } catch (err) {
      set({ lastError: (err as Error).message, loading: false });
    }
  },
}));
