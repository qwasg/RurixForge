import {
  Box,
  Braces,
  File,
  FileArchive,
  FileAudio,
  FileCode2,
  FileCog,
  FileImage,
  FileLock,
  FileTerminal,
  FileText,
  FileVideo,
  type LucideIcon,
} from 'lucide-react';

/**
 * 文件树 / 命令面板的文件类型图标(按扩展名;颜色走语义 token,克制着色,同类同色)。
 * 返回的 tone 是 Tailwind 类名(静态字面量,供 JIT 扫描)。
 */
export interface FileIconSpec {
  Icon: LucideIcon;
  tone: string;
}

const CODE: FileIconSpec = { Icon: FileCode2, tone: 'text-info' };
const RUST: FileIconSpec = { Icon: FileCode2, tone: 'text-acc' };
const DATA: FileIconSpec = { Icon: Braces, tone: 'text-warn' };
const CONFIG: FileIconSpec = { Icon: FileCog, tone: 'text-fg-3' };
const DOC: FileIconSpec = { Icon: FileText, tone: 'text-fg-3' };
const IMAGE: FileIconSpec = { Icon: FileImage, tone: 'text-sage' };
const VIDEO: FileIconSpec = { Icon: FileVideo, tone: 'text-acc' };
const AUDIO: FileIconSpec = { Icon: FileAudio, tone: 'text-acc' };
const MODEL: FileIconSpec = { Icon: Box, tone: 'text-info' };
const SHELL: FileIconSpec = { Icon: FileTerminal, tone: 'text-fg-3' };
const ARCHIVE: FileIconSpec = { Icon: FileArchive, tone: 'text-fg-4' };
const LOCK: FileIconSpec = { Icon: FileLock, tone: 'text-fg-4' };
const PLAIN: FileIconSpec = { Icon: File, tone: 'text-fg-4' };

const BY_EXT: Record<string, FileIconSpec> = {
  ts: CODE, tsx: CODE, js: CODE, jsx: CODE, mjs: CODE, cjs: CODE, go: CODE, py: CODE, css: CODE, html: CODE, htm: CODE,
  rs: RUST, rx: RUST,
  json: DATA, rxscene: DATA, rxgraph: DATA, rxmat: DATA, rxsprite: DATA, meta: DATA, jsonl: DATA,
  toml: CONFIG, yaml: CONFIG, yml: CONFIG, ini: CONFIG, env: CONFIG, gitignore: CONFIG,
  md: DOC, markdown: DOC, txt: DOC, log: DOC,
  png: IMAGE, jpg: IMAGE, jpeg: IMAGE, gif: IMAGE, webp: IMAGE, svg: IMAGE, ico: IMAGE, bmp: IMAGE,
  mp4: VIDEO, webm: VIDEO, mov: VIDEO,
  wav: AUDIO, mp3: AUDIO, ogg: AUDIO, flac: AUDIO,
  glb: MODEL, gltf: MODEL, fbx: MODEL, obj: MODEL, blend: MODEL,
  ps1: SHELL, psm1: SHELL, sh: SHELL, bash: SHELL, bat: SHELL, cmd: SHELL,
  zip: ARCHIVE, '7z': ARCHIVE, tar: ARCHIVE, gz: ARCHIVE, rar: ARCHIVE,
  lock: LOCK,
};

export function fileIconFor(name: string): FileIconSpec {
  const lower = name.toLowerCase();
  const dot = lower.lastIndexOf('.');
  const ext = dot >= 0 ? lower.slice(dot + 1) : lower;
  return BY_EXT[ext] ?? PLAIN;
}
