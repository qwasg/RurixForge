/** vite.config.ts 构建期注入 package.json 版本;测试等未注入环境回落 dev。 */
declare const __APP_VERSION__: string | undefined;

export const APP_VERSION: string = typeof __APP_VERSION__ === 'string' ? __APP_VERSION__ : 'dev';
