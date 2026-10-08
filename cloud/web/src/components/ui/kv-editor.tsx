import { ArrowRight, Plus, Trash2 } from 'lucide-react';
import { Button } from './button';
import { Input } from './form';

export interface KvRow {
  id: number;
  key: string;
  value: string;
}

let nextRowId = 1;

export function kvRow(key = '', value = ''): KvRow {
  return { id: nextRowId++, key, value };
}

export function recordToRows(record: Record<string, string> | null | undefined): KvRow[] {
  return Object.entries(record ?? {}).map(([k, v]) => kvRow(k, v));
}

/** 丢弃键或值为空的行；重复键以后出现的为准。 */
export function rowsToRecord(rows: KvRow[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const r of rows) {
    const k = r.key.trim();
    const v = r.value.trim();
    if (k && v) out[k] = v;
  }
  return out;
}

/** 键 → 值编辑器（账号的 modelMapping：对外模型 ID → 上游模型名）。 */
export function KeyValueEditor({
  rows,
  onChange,
  keyPlaceholder = '对外模型 ID',
  valuePlaceholder = '上游模型名',
  addLabel = '添加映射',
}: {
  rows: KvRow[];
  onChange: (rows: KvRow[]) => void;
  keyPlaceholder?: string;
  valuePlaceholder?: string;
  addLabel?: string;
}) {
  const update = (id: number, patch: Partial<KvRow>) => onChange(rows.map((r) => (r.id === id ? { ...r, ...patch } : r)));
  return (
    <div className="flex flex-col gap-1.5">
      {rows.map((row, i) => (
        <div key={row.id} className="flex items-center gap-1.5">
          <Input
            value={row.key}
            placeholder={keyPlaceholder}
            aria-label={`第 ${i + 1} 行：${keyPlaceholder}`}
            className="font-mono text-xs"
            onChange={(e) => update(row.id, { key: e.target.value })}
          />
          <ArrowRight className="size-3.5 shrink-0 text-muted-foreground" />
          <Input
            value={row.value}
            placeholder={valuePlaceholder}
            aria-label={`第 ${i + 1} 行：${valuePlaceholder}`}
            className="font-mono text-xs"
            onChange={(e) => update(row.id, { value: e.target.value })}
          />
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={`删除第 ${i + 1} 行`}
            onClick={() => onChange(rows.filter((r) => r.id !== row.id))}
          >
            <Trash2 />
          </Button>
        </div>
      ))}
      <Button variant="ghost" size="sm" className="self-start" onClick={() => onChange([...rows, kvRow()])}>
        <Plus />
        {addLabel}
      </Button>
    </div>
  );
}
