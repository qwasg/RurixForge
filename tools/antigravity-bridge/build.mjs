import { spawnSync } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const dir = path.dirname(fileURLToPath(import.meta.url));
const output = path.resolve(dir, '../../data/antigravity-runtime');
mkdirSync(output, { recursive: true });
const result = spawnSync('go', ['build', '-trimpath', '-ldflags=-s -w', '-o', path.join(output, process.platform === 'win32' ? 'forge-antigravity-bridge.exe' : 'forge-antigravity-bridge'), '.'], { cwd: dir, stdio: 'inherit', windowsHide: true });
if (result.error) console.error('构建反重力连接组件需要 Go 1.26 或更新版本。');
process.exit(result.status ?? 1);
