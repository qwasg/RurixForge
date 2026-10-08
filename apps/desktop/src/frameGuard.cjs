'use strict';

// D-044 UltraPlan:试玩 Demo 是应用内第一个 iframe,Demo 代码由 Agent 生成,可 `location.href = …`
// 自行导航,而 CSP 管不住导航。子框架只许停在本机回环、与应用各源都不同、且端口不是任何应用端口的
// demo host `/u/…` 路径上;其余一律拦下(调用方不转系统浏览器)。纯函数,单测见 test/frameGuard.test.cjs。
const LOOPBACK_HOSTS = new Set(['127.0.0.1', 'localhost', '[::1]']);

/**
 * 子框架导航放行判定:raw = 目标 URL;appUrls = 托管应用壳/API 的地址,单个或数组。桌面开发态须传
 * [FORGE_DEV_URL, HOST_ORIGIN]——Vite 之外 host 仍托管构建壳与代理 API。空数组或任一项解析失败一律拒。
 */
function isAllowedSubframeUrl(raw, appUrls) {
  const list = Array.isArray(appUrls) ? appUrls : [appUrls];
  if (list.length === 0) return false;
  let url;
  let apps;
  try {
    url = new URL(raw);
    apps = list.map((u) => new URL(u));
  } catch {
    return false;
  }
  const port = (u) => u.port || (u.protocol === 'https:' ? '443' : '80');
  return (
    url.protocol === 'http:' &&
    LOOPBACK_HOSTS.has(url.hostname) &&
    !url.username &&
    !url.password &&
    // 换个回环主机名仍是应用那台服务(host 另有 frame-ancestors 'none' 兜底),同端口直接拒。
    apps.every((app) => url.origin !== app.origin && port(url) !== port(app)) &&
    url.pathname.startsWith('/u/')
  );
}

module.exports = { isAllowedSubframeUrl };
