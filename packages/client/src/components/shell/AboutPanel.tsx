import { useEffect, useState, type ReactNode } from 'react';
import { Check, Copy } from 'lucide-react';
import { APP_VERSION } from '@/lib/appVersion';
import { bridge, isDesktopBridge } from '@/lib/bridge';
import { cn } from '@/lib/cn';
import { copyText } from '@/lib/clipboard';
import { getCodexStatus } from '@/lib/forgeApi';
import { humanDuration, useSystemPolling, useSystemStore } from '@/lib/systemStore';
import ForgeLogo from '@/components/ForgeLogo';

/**
 * 关于面板(Help → 关于 弹窗与设置·关于页共用):版本与服务状态全部实测——
 * 客户端版本(构建期注入)、host / agentd(host 健康接口 + 上游探测)、Codex CLI(codex status)、
 * 桌面端运行时(preload versions)、平台与本机用户;「复制诊断信息」把同一份数据以 JSON 交给剪贴板。
 */

type CodexInfo = { state: 'loading' } | { state: 'ok'; installed: boolean; version: string | null } | { state: 'error' };

function Row({ label, desc, value, testId }: { label: string; desc?: string; value: ReactNode; testId?: string }) {
  return (
    <div className="flex items-center gap-3 border-b border-edge px-3.5 py-2.5 last:border-b-0">
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="text-[12.5px] font-medium text-fg">{label}</span>
        {desc && <span className="truncate text-[11px] text-fg-3">{desc}</span>}
      </div>
      <span data-testid={testId} className="shrink-0 text-right font-code text-[11px] text-fg-2">
        {value}
      </span>
    </div>
  );
}

export default function AboutPanel({ compact = false }: { compact?: boolean }) {
  useSystemPolling();
  const checked = useSystemStore((st) => st.checked);
  const online = useSystemStore((st) => st.online);
  const snapshotOk = useSystemStore((st) => st.snapshotOk);
  const health = useSystemStore((st) => st.health);
  const [codex, setCodex] = useState<CodexInfo>({ state: 'loading' });
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let alive = true;
    getCodexStatus()
      .then((s) => alive && setCodex({ state: 'ok', installed: s.installed, version: s.version ?? null }))
      .catch(() => alive && setCodex({ state: 'error' }));
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(t);
  }, [copied]);

  const desktop = isDesktopBridge();
  const versions = desktop ? bridge().versions : undefined;
  // 旧版 host 健康接口没有 agentd 探测字段:退回「快照经代理可达」判定
  const agentdOk = health?.agentd ? health.agentd.ok : snapshotOk;
  const pending = !checked;

  const hostText = pending
    ? '探测中…'
    : online
      ? ['在线', health?.version && `v${health.version}`, health?.uptimeSec !== undefined && `已运行 ${humanDuration(health.uptimeSec)}`]
          .filter(Boolean)
          .join(' · ')
      : '离线';
  const agentdText = pending
    ? '探测中…'
    : agentdOk
      ? ['可达', health?.agentd?.version && `v${health.agentd.version}`, health?.agentd?.uptimeSec !== undefined && `已运行 ${humanDuration(health.agentd.uptimeSec)}`]
          .filter(Boolean)
          .join(' · ')
      : '不可达';
  const codexText =
    codex.state === 'loading'
      ? '探测中…'
      : codex.state === 'error'
        ? '无法读取'
        : codex.installed
          ? codex.version
            ? `v${codex.version}`
            : '已安装'
          : '未安装';
  const desktopText = versions?.electron
    ? `Electron ${versions.electron} · Chromium ${versions.chrome ?? '?'} · Node ${versions.node ?? '?'}`
    : '浏览器模式';
  const platformText = [health?.platform ?? (desktop ? bridge().platform : navigator.platform), health?.user?.name]
    .filter(Boolean)
    .join(' · ');

  const diagnostics = () =>
    JSON.stringify(
      {
        client: APP_VERSION,
        host: online ? { version: health?.version, uptimeSec: health?.uptimeSec, node: health?.node } : 'offline',
        agentd: health?.agentd ?? { ok: agentdOk },
        codex: codex.state === 'ok' ? { installed: codex.installed, version: codex.version } : codex.state,
        desktop: versions ?? null,
        platform: health?.platform ?? null,
        userAgent: navigator.userAgent,
        time: new Date().toISOString(),
      },
      null,
      2,
    );

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-3">
        <ForgeLogo
          className={cn(
            'rounded-xl',
            compact ? 'h-12 w-12' : 'h-14 w-14',
          )}
        />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="font-serif text-[16px] font-semibold text-fg">RurixForge · 游戏引擎工作台</span>
          <span data-testid="about-client-version" className="text-[11px] text-fg-3">
            客户端 v{APP_VERSION} · Apache-2.0
          </span>
        </span>
        <button
          type="button"
          data-testid="about-copy-diagnostics"
          onClick={() => void copyText(diagnostics(), '诊断信息').then((ok) => setCopied(ok))}
          className="flex h-7 shrink-0 items-center gap-1 rounded-md border border-edge px-2 text-[11.5px] text-fg-2 transition-colors hover:bg-shell-hover"
        >
          {copied ? <Check size={12} className="text-sage" /> : <Copy size={12} />}
          {copied ? '已复制' : '复制诊断信息'}
        </button>
      </div>
      <div data-testid="about-services" className="flex flex-col rounded-[10px] border border-edge bg-shell-sunk">
        <Row label="host" desc="127.0.0.1:3080 · @forge/host" value={hostText} testId="about-host-health" />
        <Row label="agentd" desc="127.0.0.1:8103 · forge-agentd(经 host 代理)" value={agentdText} testId="about-agentd-health" />
        <Row label="Codex CLI" desc="可选的 Codex 执行引擎" value={codexText} testId="about-codex" />
        <Row label="运行环境" desc={desktop ? '桌面端' : '浏览器'} value={desktopText} testId="about-runtime" />
        <Row label="平台" desc="host 所在系统与本机用户" value={platformText || '—'} testId="about-platform" />
      </div>
    </div>
  );
}
