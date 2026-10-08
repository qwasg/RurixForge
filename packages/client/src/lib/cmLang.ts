import type { Extension } from '@codemirror/state';

/**
 * F9:文件编辑器语言分派(按扩展名 → CodeMirror 6 语言扩展)。
 * 全部动态 import 懒加载(语言包不进首屏 bundle);
 * 消费方经 Compartment 在加载完成后热插(见 CodeEditor.tsx)。
 * .rx 语法为 Rust 系(projects/demo Content/Scripts/maze.rx:#[export(c)] pub fn ... -> bool);
 * .rxscene/.rxgraph/.rxmat/.meta 均为 JSON 资产面。
 */

export type CmLangId =
  | 'javascript'
  | 'rust'
  | 'json'
  | 'markdown'
  | 'powershell'
  | 'toml'
  | 'yaml'
  | 'shell'
  | 'css'
  | 'html';

/** 扩展名 → 语言 id(纯函数,测试直断;null = 无高亮,纯文本编辑仍可用)。 */
export function langIdForPath(path: string): CmLangId | null {
  const name = path.replace(/\\/g, '/').split('/').pop() ?? '';
  const dot = name.lastIndexOf('.');
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
  switch (ext) {
    case 'ts':
    case 'tsx':
    case 'js':
    case 'jsx':
    case 'mjs':
    case 'cjs':
      return 'javascript';
    case 'rs':
    case 'rx':
      return 'rust';
    case 'json':
    case 'rxscene':
    case 'rxgraph':
    case 'rxmat':
    case 'rxshadergraph':
    case 'meta':
      return 'json';
    case 'md':
    case 'markdown':
      return 'markdown';
    case 'ps1':
    case 'psm1':
    case 'psd1':
      return 'powershell';
    case 'toml':
      return 'toml';
    case 'yaml':
    case 'yml':
      return 'yaml';
    case 'sh':
    case 'bash':
      return 'shell';
    case 'css':
      return 'css';
    case 'html':
    case 'htm':
      return 'html';
    default:
      return null;
  }
}

/** legacy-modes StreamParser → 语言扩展(css/html 无官方独立包需求面,走 legacy 轻模式)。 */
async function legacyMode(load: () => Promise<{ parser: unknown }>): Promise<Extension> {
  const [{ StreamLanguage }, { parser }] = await Promise.all([
    import('@codemirror/language'),
    load(),
  ]);
  return StreamLanguage.define(parser as Parameters<typeof StreamLanguage.define>[0]);
}

/** 语言扩展懒加载(null = 无匹配语言)。 */
export async function langExtensionForPath(path: string): Promise<Extension | null> {
  const id = langIdForPath(path);
  if (id === null) return null;
  const lower = path.toLowerCase();
  switch (id) {
    case 'javascript': {
      const { javascript } = await import('@codemirror/lang-javascript');
      return javascript({
        typescript: lower.endsWith('.ts') || lower.endsWith('.tsx'),
        jsx: lower.endsWith('.tsx') || lower.endsWith('.jsx'),
      });
    }
    case 'rust': {
      const { rust } = await import('@codemirror/lang-rust');
      return rust();
    }
    case 'json': {
      const { json } = await import('@codemirror/lang-json');
      return json();
    }
    case 'markdown': {
      const { markdown } = await import('@codemirror/lang-markdown');
      return markdown();
    }
    case 'powershell':
      return legacyMode(async () => {
        const { powerShell } = await import('@codemirror/legacy-modes/mode/powershell');
        return { parser: powerShell };
      });
    case 'toml':
      return legacyMode(async () => {
        const { toml } = await import('@codemirror/legacy-modes/mode/toml');
        return { parser: toml };
      });
    case 'yaml':
      return legacyMode(async () => {
        const { yaml } = await import('@codemirror/legacy-modes/mode/yaml');
        return { parser: yaml };
      });
    case 'shell':
      return legacyMode(async () => {
        const { shell } = await import('@codemirror/legacy-modes/mode/shell');
        return { parser: shell };
      });
    case 'css':
      return legacyMode(async () => {
        const { css } = await import('@codemirror/legacy-modes/mode/css');
        return { parser: css };
      });
    case 'html': {
      // legacy html 混模式依赖多包;html 文件走 xml 轻模式(标签/属性面足够)。
      return legacyMode(async () => {
        const { xml } = await import('@codemirror/legacy-modes/mode/xml');
        return { parser: xml };
      });
    }
  }
}
