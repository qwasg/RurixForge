import type { ReactNode } from 'react';
import { jumpToEntity } from '@/lib/entityJump';

/**
 * UI 融合波 C2:正文 #id 实体引用链接化(保守:仅 #数字,1–7 位)。
 * 渲染为 accent 链接;点击经 jumpToEntity 守卫(仅当 id 命中当前场景实体才跳)。
 * 供 MarkdownFlat 行内文本段使用;代码块/表格不链接化(保持 mono 原文)。
 */
const ENTITY_REF_RE = /#(\d{1,7})/g;

export default function EntityRefText({ text }: { text: string }) {
  const out: ReactNode[] = [];
  let last = 0;
  let m: RegExpExecArray | null;
  ENTITY_REF_RE.lastIndex = 0;
  while ((m = ENTITY_REF_RE.exec(text)) !== null) {
    if (m.index > last) out.push(text.slice(last, m.index));
    const id = Number(m[1]);
    out.push(
      <button
        key={`${m.index}-${id}`}
        type="button"
        data-testid={`entity-ref-${id}`}
        title="在编辑器中选中并聚焦"
        onClick={(e) => {
          e.stopPropagation();
          jumpToEntity(id);
        }}
        className="font-medium text-acc underline decoration-acc/40 underline-offset-2 hover:decoration-acc"
      >
        #{id}
      </button>,
    );
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return <>{out}</>;
}
