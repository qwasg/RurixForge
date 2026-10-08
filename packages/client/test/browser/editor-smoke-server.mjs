import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
process.chdir(root);
const server = await createServer({ root, configFile: path.join(root, 'vite.config.ts'), server: { host: '127.0.0.1', port: Number(process.argv[2] ?? 5197), strictPort: true, proxy: { '/api/forge': { target: process.argv[3] ?? 'http://127.0.0.1:51142', changeOrigin: true } } } });
await server.listen(); server.printUrls();
