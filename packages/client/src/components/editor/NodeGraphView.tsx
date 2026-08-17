import { useEffect, useState } from 'react';
import { RefreshCw, Save } from 'lucide-react';
import { cn } from '@/lib/cn';
import {
  useGraphStore,
  type ExposedProp,
  type GraphNode,
  type GraphValue,
  type ValueSource,
} from '@/lib/graphStore';

/**
 * NodeGraph 面板(07 §1 G 区,与 Viewport 同位页签;10 §5)。
 * 只做查看/微调(常量、暴露属性默认值)/保存;从零生成与拖线重连走 Chat agent
 * (logic-blueprint-gen),本面板不做拖新节点/拖线连接。
 * 渲染数据全部来自 graph_get 真实响应;保存即 graph_validate 全图校验,不过不落盘。
 */

const NODE_W = 140;
const BAR_H = 3; // 节点头色条高
const TITLE_H = 20; // 节点标题行高
const ROW_H = 18; // 输入行高

/** 节点头色条:event.* 绿 / flow.* 蓝 / 其余灰(照 UE 蓝图直觉) */
function headerBar(type: string): string {
  if (type.startsWith('event.')) return 'bg-accent-green';
  if (type.startsWith('flow.')) return 'bg-accent-blue';
  return 'bg-muted-faint';
}

/** 值 → 显示文本(字符串不加引号,其余 JSON 形态) */
function valueText(v: GraphValue): string {
  return typeof v === 'string' ? v : JSON.stringify(v);
}

/** 编辑提交:优先按 JSON 字面量解析(90/true/[1,2,3]),失败按字符串 */
function parseLiteral(text: string): GraphValue {
  const t = text.trim();
  if (t === '') return '';
  try {
    return JSON.parse(t) as GraphValue;
  } catch {
    return t;
  }
}

// ---------- 常量内联编辑(点击变 input,回车/失焦提交 editConst) ----------

function ConstCell({
  nodeId,
  pin,
  value,
}: {
  nodeId: string;
  pin: string;
  value: GraphValue;
}) {
  const editConst = useGraphStore((s) => s.editConst);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(valueText(value));
  useEffect(() => setDraft(valueText(value)), [value]);

  const commit = () => {
    setEditing(false);
    const next = parseLiteral(draft);
    if (next !== value) editConst(nodeId, pin, next);
  };

  if (editing) {
    return (
      <input
        autoFocus
        data-testid={`const-input-${nodeId}-${pin}`}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit();
          if (e.key === 'Escape') {
            setDraft(valueText(value));
            setEditing(false);
          }
        }}
        className="w-full min-w-0 rounded border border-line bg-white px-1 py-px font-mono text-2xs text-ink outline-none focus:border-muted-faint"
      />
    );
  }
  return (
    <span
      role="button"
      tabIndex={0}
      data-testid={`const-${nodeId}-${pin}`}
      title="点击编辑常量"
      onClick={() => {
        setDraft(valueText(value));
        setEditing(true);
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter') {
          setDraft(valueText(value));
          setEditing(true);
        }
      }}
      className="cursor-text truncate rounded px-1 font-mono text-ink-soft hover:bg-panel-hover"
    >
      {valueText(value)}
    </span>
  );
}

/** 单个数据输入行:const 可编辑 / ref 暴露属性引用 / node+pin 数据边来源 */
function InputRow({ nodeId, pin, src }: { nodeId: string; pin: string; src: ValueSource }) {
  return (
    <div className="flex h-[18px] items-center gap-1 text-2xs">
      <span className="shrink-0 text-muted">{pin}</span>
      <span className="min-w-0 flex-1 truncate text-right">
        {'const' in src ? (
          <ConstCell nodeId={nodeId} pin={pin} value={src.const} />
        ) : 'ref' in src ? (
          <span className="truncate font-mono text-accent-blue" title={`暴露属性 ${src.ref}`}>
            = {src.ref}
          </span>
        ) : (
          <span
            className="truncate font-mono text-muted-faint"
            title={`数据边 ← ${src.node}.${src.pin}`}
          >
            ← {src.node}.{src.pin}
          </span>
        )}
      </span>
    </div>
  );
}

// ---------- 节点卡片 ----------

function NodeCard({ node, hasError }: { node: GraphNode; hasError: boolean }) {
  const inputs = Object.entries(node.inputs ?? {});
  return (
    <div
      data-graph-node={node.id}
      className={cn(
        'absolute select-none rounded-md border bg-white shadow-composer',
        hasError ? 'border-red-500' : 'border-line',
      )}
      style={{ left: node.pos[0], top: node.pos[1], width: NODE_W }}
    >
      <div className={cn('h-[3px] rounded-t-md', headerBar(node.type))} />
      <div
        className="truncate px-1.5 text-2xs font-medium leading-[20px] text-ink"
        title={`${node.type} (${node.id})`}
      >
        {node.type}
      </div>
      {inputs.length > 0 && (
        <div className="border-t border-line-soft px-1.5 py-0.5">
          {inputs.map(([pin, src]) => (
            <InputRow key={pin} nodeId={node.id} pin={pin} src={src} />
          ))}
        </div>
      )}
    </div>
  );
}

// ---------- 暴露属性表 ----------

function ExposedPropRow({ prop }: { prop: ExposedProp }) {
  const editExposedDefault = useGraphStore((s) => s.editExposedDefault);
  const [draft, setDraft] = useState(valueText(prop.default));
  useEffect(() => setDraft(valueText(prop.default)), [prop.default]);

  const commit = () => {
    const next = parseLiteral(draft);
    if (next !== prop.default) editExposedDefault(prop.name, next);
    else setDraft(valueText(prop.default));
  };

  return (
    <div className="flex items-center gap-1 px-2 py-0.5">
      <span className="min-w-0 flex-1 truncate text-2xs text-ink-soft" title={prop.name}>
        {prop.name}
        <span className="ml-1 text-muted-faint">{prop.kind}</span>
      </span>
      <input
        data-testid={`exposed-${prop.name}`}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
          if (e.key === 'Escape') setDraft(valueText(prop.default));
        }}
        className="w-[64px] shrink-0 rounded border border-line bg-white px-1 py-px font-mono text-2xs text-ink outline-none focus:border-muted-faint"
      />
    </div>
  );
}

// ---------- 画布几何 ----------

interface EdgeLine {
  key: string;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
}

/** 执行边(edges 数组,实线) */
function execEdgeLines(nodes: GraphNode[], edges: Array<{ from: [string, string]; to: [string, string] }>): EdgeLine[] {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const out: EdgeLine[] = [];
  for (const e of edges) {
    const a = byId.get(e.from[0]);
    const b = byId.get(e.to[0]);
    if (!a || !b) continue; // 悬空端如实跳过(校验器会以 GRAPH_* 报出)
    out.push({
      key: `exec-${e.from[0]}.${e.from[1]}-${e.to[0]}.${e.to[1]}`,
      x1: a.pos[0] + NODE_W,
      y1: a.pos[1] + BAR_H + TITLE_H / 2,
      x2: b.pos[0],
      y2: b.pos[1] + BAR_H + TITLE_H / 2,
    });
  }
  return out;
}

/** 数据边(inputs 内 node+pin 引用,虚线贝塞尔) */
function dataEdgeLines(nodes: GraphNode[]): EdgeLine[] {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const out: EdgeLine[] = [];
  for (const n of nodes) {
    const pins = Object.keys(n.inputs ?? {});
    pins.forEach((pin, i) => {
      const src = (n.inputs ?? {})[pin];
      if (!('node' in src)) return;
      const s = byId.get(src.node);
      if (!s) return;
      out.push({
        key: `data-${src.node}.${src.pin}-${n.id}.${pin}`,
        x1: s.pos[0] + NODE_W,
        y1: s.pos[1] + BAR_H + TITLE_H + ROW_H / 2,
        x2: n.pos[0],
        y2: n.pos[1] + BAR_H + TITLE_H + i * ROW_H + ROW_H / 2,
      });
    });
  }
  return out;
}

// ---------- 主视图 ----------

export default function NodeGraphView() {
  const graph = useGraphStore((s) => s.graph);
  const graphPath = useGraphStore((s) => s.graphPath);
  const errors = useGraphStore((s) => s.errors);
  const dirty = useGraphStore((s) => s.dirty);
  const loading = useGraphStore((s) => s.loading);
  const lastError = useGraphStore((s) => s.lastError);
  const lastSaved = useGraphStore((s) => s.lastSaved);
  const loadByPath = useGraphStore((s) => s.loadByPath);
  const save = useGraphStore((s) => s.save);

  const [pathDraft, setPathDraft] = useState('');

  const errorNodeIds = new Set(errors.map((e) => e.nodeId).filter((x): x is string => !!x));
  const execLines = graph ? execEdgeLines(graph.nodes, graph.edges) : [];
  const dataLines = graph ? dataEdgeLines(graph.nodes) : [];
  const canvasW = graph
    ? Math.max(640, ...graph.nodes.map((n) => n.pos[0] + NODE_W + 80))
    : 640;
  const canvasH = graph
    ? Math.max(
        360,
        ...graph.nodes.map(
          (n) => n.pos[1] + BAR_H + TITLE_H + Object.keys(n.inputs ?? {}).length * ROW_H + 90,
        ),
      )
    : 360;

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-panel" aria-label="NodeGraph">
      {/* 顶栏:图名/路径/dirty 点 + 路径输入 + 重载/保存 + 状态行 */}
      <div className="flex shrink-0 items-center gap-1.5 border-b border-line-soft bg-white px-2 py-1">
        <span className="shrink-0 text-2xs text-muted-faint">NodeGraph</span>
        {graph && (
          <span className="flex min-w-0 items-center gap-1 text-2xs text-ink-soft">
            <span className="truncate font-medium">{graph.name}</span>
            <span className="truncate text-muted-faint">{graphPath}</span>
            {dirty && <span title="未保存修改" className="h-1.5 w-1.5 shrink-0 rounded-full bg-amber-500" />}
          </span>
        )}
        <input
          data-graph-path-input
          title="图路径"
          value={pathDraft}
          onChange={(e) => setPathDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void loadByPath(pathDraft);
          }}
          placeholder="Content/Graphs/xxx.rxgraph"
          className="w-[220px] shrink-0 rounded-md border border-line bg-white px-2 py-0.5 font-mono text-2xs text-ink outline-none placeholder:text-muted-faint focus:border-muted-faint"
        />
        <button
          type="button"
          data-graph-load
          disabled={loading || pathDraft.trim() === ''}
          onClick={() => void loadByPath(pathDraft)}
          className="shrink-0 rounded-md border border-line bg-white px-2 py-0.5 text-2xs text-ink-soft transition-colors hover:bg-panel-hover disabled:opacity-40"
        >
          加载
        </button>
        <button
          type="button"
          title="重载"
          disabled={loading || !graph || !graphPath}
          onClick={() => graphPath && void loadByPath(graphPath)}
          className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-muted transition-colors hover:bg-panel-hover hover:text-ink-soft disabled:opacity-40 disabled:hover:bg-transparent"
        >
          <RefreshCw size={12} strokeWidth={1.8} />
        </button>
        <button
          type="button"
          title="保存"
          disabled={loading || !graph || !dirty}
          onClick={() => void save()}
          className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-muted transition-colors hover:bg-panel-hover hover:text-ink-soft disabled:opacity-40 disabled:hover:bg-transparent"
        >
          <Save size={12} strokeWidth={1.8} />
        </button>
        <span className="flex-1" />
        {/* 状态行:错误红 / 保存成功绿 / 加载中灰,简朴如实 */}
        {lastError ? (
          <span className="truncate text-2xs text-red-600" title={lastError}>
            {lastError}
          </span>
        ) : lastSaved ? (
          <span className="truncate text-2xs text-accent-green">已保存 {lastSaved}</span>
        ) : loading ? (
          <span className="text-2xs text-muted-faint">加载中…</span>
        ) : null}
      </div>

      {graph ? (
        <div className="flex min-h-0 flex-1">
          {/* 画布:SVG 网格 + 边,HTML 节点卡片(交互编辑) */}
          <div className="min-h-0 flex-1 overflow-auto">
            <div className="relative" style={{ width: canvasW, height: canvasH }}>
              <svg width={canvasW} height={canvasH} className="absolute inset-0">
                <defs>
                  <pattern id="ng-grid" width="20" height="20" patternUnits="userSpaceOnUse">
                    <path d="M 20 0 L 0 0 0 20" fill="none" stroke="#efedea" strokeWidth="1" />
                  </pattern>
                </defs>
                <rect width="100%" height="100%" fill="url(#ng-grid)" />
                {dataLines.map((l) => {
                  const dx = Math.max(40, Math.abs(l.x2 - l.x1) / 2);
                  return (
                    <path
                      key={l.key}
                      data-graph-edge="data"
                      d={`M ${l.x1} ${l.y1} C ${l.x1 + dx} ${l.y1}, ${l.x2 - dx} ${l.y2}, ${l.x2} ${l.y2}`}
                      fill="none"
                      stroke="#2563eb"
                      strokeWidth="1.2"
                      strokeDasharray="4 3"
                    />
                  );
                })}
                {execLines.map((l) => (
                  <line
                    key={l.key}
                    data-graph-edge="exec"
                    x1={l.x1}
                    y1={l.y1}
                    x2={l.x2}
                    y2={l.y2}
                    stroke="#9c9994"
                    strokeWidth="1.5"
                  />
                ))}
              </svg>
              {graph.nodes.map((n) => (
                <NodeCard key={n.id} node={n} hasError={errorNodeIds.has(n.id)} />
              ))}
            </div>
          </div>

          {/* 暴露属性表(default 可编辑) */}
          <aside className="w-[190px] shrink-0 overflow-y-auto border-l border-line-soft bg-white">
            <p className="px-2 pb-1 pt-2 text-2xs text-muted-faint">Exposed</p>
            {graph.exposedProps.length === 0 && (
              <p className="px-2 text-2xs text-muted-faint">(无暴露属性)</p>
            )}
            {graph.exposedProps.map((p) => (
              <ExposedPropRow key={p.name} prop={p} />
            ))}
          </aside>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 items-center justify-center">
          <p className="max-w-[440px] px-4 text-center text-xs leading-5 text-muted-faint">
            未加载图:在 Hierarchy 选中带 Script 组件(graphRef)的实体,或在上方输入图路径加载。
            <br />
            从零生成走 Chat agent(logic-blueprint-gen);本面板定位 = 审阅 / 微调 / 改常量(10 §5)。
          </p>
        </div>
      )}

      {/* 校验错误条(保存时 graph_validate 全量返回;错误节点红框) */}
      {errors.length > 0 && (
        <div
          data-testid="graph-errors"
          className="max-h-[96px] shrink-0 overflow-y-auto border-t border-line-soft bg-white px-2 py-1"
        >
          {errors.map((e, i) => (
            <p key={i} className="truncate py-px font-mono text-2xs text-red-600" title={e.message}>
              [{e.code}]{e.nodeId ? ` ${e.nodeId}` : ''} {e.message}
            </p>
          ))}
        </div>
      )}
    </div>
  );
}
