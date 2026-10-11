import { buildServer } from './server.js';
import type { HostConfig } from './plugins/config.js';
import type { Logger } from './plugins/logger.js';

/**
 * 进程入口:启动 web profile 并处理优雅退出。
 *
 * FORGE_HOST_PORT 覆盖监听端口(缺省 3080)。加这个口子是为了让栈级冒烟能在开发者
 * 已开着 dev 实例的机器上并排跑——否则验收脚本只能要求先关掉 dev 环境。
 * 非法值(非正整数)按缺省处理,不静默监听到 0 号随机端口上让调用方找不着。
 * FORGE_DATA_DIR(安装版由桌面端设置)覆盖数据目录,缺省 <cwd>/data。
 */
const envPort = Number(process.env.FORGE_HOST_PORT);
const portPatch = Number.isInteger(envPort) && envPort > 0 && envPort < 65536
  ? { port: envPort }
  : {};
const dataPatch = process.env.FORGE_DATA_DIR ? { dataDir: process.env.FORGE_DATA_DIR } : {};
const server = buildServer({ ...portPatch, ...dataPatch });
const config = server.ctx.get<HostConfig>('config');
const logger = server.ctx.get<Logger>('logger');

const port = await server.listen(config.port);
logger.info(`forge-host listening at http://${config.host}:${port}`);

let closing = false;
async function shutdown(signal: string): Promise<void> {
  if (closing) return;
  closing = true;
  logger.info(`received ${signal}, closing...`);
  await server.close();
  process.exit(0);
}

process.on('SIGINT', () => void shutdown('SIGINT'));
process.on('SIGTERM', () => void shutdown('SIGTERM'));

// 桌面端以管道作 stdin 拉起 host:管道断开 = 桌面端已退出(含崩溃),随之退出,不残留占端口。
if (process.env.FORGE_EXIT_ON_STDIN_EOF) {
  process.stdin.on('end', () => void shutdown('stdin closed'));
  process.stdin.on('error', () => void shutdown('stdin closed'));
  process.stdin.resume();
}
