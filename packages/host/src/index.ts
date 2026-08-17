import { buildServer } from './server.js';
import type { HostConfig } from './plugins/config.js';
import type { Logger } from './plugins/logger.js';

/** 进程入口:启动 web profile 并处理优雅退出 */
const server = buildServer();
const config = server.ctx.get<HostConfig>('config');
const logger = server.ctx.get<Logger>('logger');

const port = await server.listen(config.port);
logger.info(`forge-host listening at http://${config.host}:${port}`);

async function shutdown(signal: string): Promise<void> {
  logger.info(`received ${signal}, closing...`);
  await server.close();
  process.exit(0);
}

process.on('SIGINT', () => void shutdown('SIGINT'));
process.on('SIGTERM', () => void shutdown('SIGTERM'));
