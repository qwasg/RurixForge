import { describe, expect, it } from 'vitest';
import type { WorkspaceGitResp } from '@/lib/forgeApi';
import { buildGitIndex, gitStatusOf } from '@/lib/gitStore';

const STATUS: WorkspaceGitResp = {
  isRepo: true,
  branch: 'main',
  files: [
    { path: 'src/lib/a.ts', status: 'M', staged: false, dir: false, insertions: 3, deletions: 1, origPath: null },
    { path: 'src/new.ts', status: 'A', staged: true, dir: false, insertions: 10, deletions: 0, origPath: null },
    { path: 'assets/raw', status: 'U', staged: false, dir: true, insertions: null, deletions: null, origPath: null },
  ],
};

describe('buildGitIndex / gitStatusOf(文件树改动标记)', () => {
  it('直接命中给状态字母;祖先目录进 dirty 集合', () => {
    const idx = buildGitIndex(STATUS);
    expect(gitStatusOf(idx, 'src/lib/a.ts')).toBe('M');
    expect(gitStatusOf(idx, 'src/new.ts')).toBe('A');
    expect(idx.dirty.has('src')).toBe(true);
    expect(idx.dirty.has('src/lib')).toBe(true);
    expect(idx.dirty.has('assets')).toBe(true);
    expect(gitStatusOf(idx, 'src/untouched.ts')).toBeNull();
  });

  it('未跟踪目录之下的文件继承 U,目录本身也标 U', () => {
    const idx = buildGitIndex(STATUS);
    expect(gitStatusOf(idx, 'assets/raw')).toBe('U');
    expect(gitStatusOf(idx, 'assets/raw/deep/x.png')).toBe('U');
    expect(gitStatusOf(idx, 'assets/other.png')).toBeNull();
  });

  it('非仓库 / 空状态 → 空索引', () => {
    const idx = buildGitIndex(null);
    expect(idx.entries.size).toBe(0);
    expect(gitStatusOf(buildGitIndex({ isRepo: false }), 'a.ts')).toBeNull();
  });
});
