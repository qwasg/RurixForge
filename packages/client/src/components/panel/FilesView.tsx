import { useState } from 'react';
import { BookOpen, ChevronDown, ChevronRight, File, Folder } from 'lucide-react';
import { FILE_TREE } from '@/lib/mock';
import type { FileNode } from '@/lib/types';

/** md 用书本图标,.gitattributes/.gitignore 用橙色图标,其余通用文件图标。 */
function FileIcon({ name }: { name: string }) {
  if (name === '.gitattributes' || name === '.gitignore') {
    return <File size={13} className="shrink-0 text-[#f05133]" />;
  }
  if (name.endsWith('.md')) {
    return <BookOpen size={13} className="shrink-0 text-muted" />;
  }
  return <File size={13} className="shrink-0 text-muted-faint" />;
}

interface NodeRowProps {
  node: FileNode;
  depth: number;
  path: string;
  expanded: ReadonlySet<string>;
  onToggle: (path: string) => void;
}

function NodeRow({ node, depth, path, expanded, onToggle }: NodeRowProps) {
  const isDir = node.type === 'dir';
  const open = isDir && expanded.has(path);

  return (
    <>
      <button
        type="button"
        onClick={isDir ? () => onToggle(path) : undefined}
        className="flex w-full items-center gap-1 rounded py-[3px] pr-2 text-left text-xs text-ink-soft transition-colors hover:bg-panel-hover"
        style={{ paddingLeft: depth * 12 + 4 }}
      >
        {isDir ? (
          open ? (
            <ChevronDown size={12} className="shrink-0 text-muted-faint" />
          ) : (
            <ChevronRight size={12} className="shrink-0 text-muted-faint" />
          )
        ) : (
          <span className="w-3 shrink-0" />
        )}
        {isDir ? <Folder size={13} className="shrink-0 text-muted" /> : <FileIcon name={node.name} />}
        <span className="truncate">{node.name}</span>
      </button>
      {open &&
        node.children?.map((child) => (
          <NodeRow
            key={child.name}
            node={child}
            depth={depth + 1}
            path={`${path}/${child.name}`}
            expanded={expanded}
            onToggle={onToggle}
          />
        ))}
    </>
  );
}

/** Files 视图:渲染 mock.FILE_TREE,文件夹点击展开/收起,文件点击无操作。 */
export default function FilesView() {
  const root = FILE_TREE[0];
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set([root.name]));

  const onToggle = (path: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div className="shrink-0 px-3 pb-1 pt-2.5 text-xs text-muted">{root.name}</div>
      <div className="min-h-0 flex-1 overflow-auto px-1 pb-2">
        {root.children?.map((child) => (
          <NodeRow
            key={child.name}
            node={child}
            depth={0}
            path={`${root.name}/${child.name}`}
            expanded={expanded}
            onToggle={onToggle}
          />
        ))}
      </div>
    </div>
  );
}
