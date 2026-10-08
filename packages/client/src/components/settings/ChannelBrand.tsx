import { Clock3 } from 'lucide-react';
import type { QuotaWindow } from '@/lib/channelApi';

export const channelBrands = {
  codex: { title: 'Codex', company: 'OpenAI', logo: '/channel-logos/codex.png', desc: 'ChatGPT 订阅 · 官方 CLI', login: 'ChatGPT 官方授权', note: '官方网页授权 · CLI 托管凭证' },
  antigravity: { title: 'Antigravity', company: 'Google', logo: '/channel-logos/antigravity.svg', desc: '反重力 · Google AI 订阅', login: 'Google 网页授权', note: 'Google 官方 OAuth · 本机加密' },
  kimi: { title: 'Kimi Code', company: 'Moonshot AI', logo: '/channel-logos/kimi.png', desc: 'Kimi 会员 · 官方编程订阅', login: 'Kimi 官方授权', note: '官方 OAuth · 自动续期' },
  glm: { title: 'GLM Coding', company: '智谱 Zhipu', logo: '/channel-logos/glm.png', desc: 'GLM · Coding Plan 订阅', login: '官方订阅登录', note: '官网登录 · 绑定套餐 Key' },
} as const;

export type ChannelBrandId = keyof typeof channelBrands;

export function ChannelBrandHeader({ channel, badge, connected }: { channel: ChannelBrandId; badge: string; connected: boolean }) {
  const brand = channelBrands[channel];
  return <header className="channel-brand">
    <div className="channel-cover" aria-hidden="true">
      <img className="channel-cover-image" src={`/channel-art/${channel}.png`} alt="" width={1672} height={941} />
    </div>
    <div className="channel-brand-top"><span className="channel-company">{brand.company}</span><span className={`channel-status${connected ? ' is-connected' : ''}`}><i />{badge}</span></div>
    <div className="channel-brand-name"><span className="channel-logo"><img src={brand.logo} alt={`${brand.company} 官方图标`} width={28} height={28} data-testid={`channel-logo-${channel}`} /></span><h3>{brand.title}</h3></div>
    <p>{brand.desc}</p>
  </header>;
}

function resetTime(raw: QuotaWindow['resetsAt']): string | null {
  if (raw === undefined) return null;
  const date = new Date(typeof raw === 'number' ? (raw < 1e11 ? raw * 1000 : raw) : raw);
  return Number.isNaN(date.getTime()) ? null : `${date.toLocaleString('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' })} 重置`;
}

export function QuotaMeter({ window: bucket }: { window: QuotaWindow }) {
  const left = Math.max(0, Math.min(100, 100 - bucket.usedPercent));
  return <div className="channel-quota-window">
    <div className="channel-quota-caption"><span>{bucket.label}</span><span>剩余</span></div>
    <div className="channel-quota-value">{Math.round(left)}<span>%</span></div>
    <div className={`channel-meter${left <= 10 ? ' channel-meter-low' : ''}`} role="progressbar" aria-label={`${bucket.label}剩余额度`} aria-valuemin={0} aria-valuemax={100} aria-valuenow={left}><div style={{ width: `${left}%` }} /></div>
    <div className="channel-reset"><Clock3 size={9} />{resetTime(bucket.resetsAt) ?? '重置时间待同步'}</div>
  </div>;
}
