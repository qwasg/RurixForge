import { useEffect, useState } from 'react';
import { apiGet } from '@/lib/forgeApi';
import { SetCard, SetH1, SetRow, SetSectionLabel } from './controls';

/**
 * F7 wave.5 关于页:RurixForge 版本 + host/agentd 地址 + health 实测状态。
 * host /api/forge/health(host 自有,不经代理);agentd 可达性经 design-snapshot 实测
 * (代理 502 = agentd 不可达,如实呈现)。
 */
export default function AboutPage() {
  const [host, setHost] = useState<{ ok: boolean; version?: string; uptime?: number }>({ ok: false });
  const [agentd, setAgentd] = useState<boolean | null>(null);

  useEffect(() => {
    let stop = false;
    (async () => {
      try {
        const h = await apiGet<{ version?: string; uptimeSec?: number }>('/api/forge/health');
        if (!stop) setHost({ ok: true, version: h.version, uptime: h.uptimeSec });
      } catch {
        if (!stop) setHost({ ok: false });
      }
      try {
        await apiGet('/api/forge/design-snapshot');
        if (!stop) setAgentd(true);
      } catch {
        if (!stop) setAgentd(false);
      }
    })();
    return () => {
      stop = true;
    };
  }, []);

  return (
    <div data-testid="settings-page-about" className="flex flex-col">
      <SetH1>关于</SetH1>
      <SetCard>
        <div className="flex items-center gap-3 px-4 py-4">
          <span className="flex h-12 w-12 shrink-0 items-center justify-center rounded-xl bg-fg font-serif text-[22px] text-fg-inv">
            铸
          </span>
          <span className="flex flex-col">
            <span className="font-serif text-[16px] font-semibold text-fg">RurixForge · 游戏引擎工作台</span>
            <span className="text-[11px] text-fg-4">v0.1.0 · F7 wave.5 workbench 与设置</span>
          </span>
        </div>
      </SetCard>
      <SetSectionLabel>服务</SetSectionLabel>
      <SetCard testId="about-services">
        <SetRow
          title="host"
          desc="127.0.0.1:3080(@forge/host)"
          control={
            <span className="font-code text-[11px] text-fg-3" data-testid="about-host-health">
              {host.ok ? `在线${host.version ? ` · v${host.version}` : ''}` : '离线'}
            </span>
          }
        />
        <SetRow
          title="agentd"
          desc="127.0.0.1:8103(forge-agentd,经 host 代理)"
          last
          control={
            <span className="font-code text-[11px] text-fg-3" data-testid="about-agentd-health">
              {agentd === null ? '探测中…' : agentd ? '可达' : '不可达'}
            </span>
          }
        />
      </SetCard>
      <SetSectionLabel>技术栈</SetSectionLabel>
      <SetCard>
        <SetRow title="前端" desc="React 18 + Tailwind + zustand + vite" last={false} />
        <SetRow title="外观参考" desc="Moonlit Agent IDE(主题派色逐行移植)" last />
      </SetCard>
    </div>
  );
}
