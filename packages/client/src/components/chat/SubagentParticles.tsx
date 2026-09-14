import type { CSSProperties } from 'react';
import { cn } from '@/lib/cn';
import type { BlockStatus } from '@/lib/timeline';

/**
 * 子代理状态粒子(取代原 Bot 图标):核心点 + 6 颗环绕粒子,色走状态点 token。
 * - running:粒子绕核心公转(外圈顺时针 / 内圈逆时针,周期错峰)+ 明灭呼吸 + 核心光晕;
 * - done:粒子静止内收成规整环,核心实心(dot-done);
 * - error:粒子静止外散(炸开态);2026-09-03 用户指令「报错不需特别标明」后只改形不改色
 *   (原 dot-blocked 红下线,与 idle 同灰,见 styles/index.css)。
 * 样式/动画在 styles/index.css(.forge-particles;prefers-reduced-motion 一律静止)。
 */

/** 轨道参数:角度 deg / 半径 px / 公转周期 s / 明灭周期 s / 负延迟 s(错峰起相) / 方向。 */
const ORBITS = [
  { a: 12, r: 5.1, t: 3.2, tw: 1.9, d: -0.2, dir: 'cw' },
  { a: 96, r: 4.5, t: 2.6, tw: 1.5, d: -0.9, dir: 'cw' },
  { a: 188, r: 5.3, t: 3.7, tw: 2.1, d: -0.5, dir: 'cw' },
  { a: 268, r: 4.3, t: 2.9, tw: 1.7, d: -1.3, dir: 'cw' },
  { a: 148, r: 2.7, t: 2.2, tw: 1.3, d: -0.7, dir: 'ccw' },
  { a: 320, r: 2.9, t: 2.5, tw: 1.6, d: -0.3, dir: 'ccw' },
] as const;

/** 参数基准盒宽;size 传其它值时半径/点径等比缩放。 */
const BASE = 14;

export default function SubagentParticles({
  status,
  size = BASE,
  className,
}: {
  status: BlockStatus;
  size?: number;
  className?: string;
}) {
  const k = size / BASE;
  const px = (v: number) => `${Math.round(v * k * 100) / 100}px`;
  return (
    <span
      data-testid="subagent-particles"
      data-state={status}
      aria-hidden="true"
      className={cn('forge-particles', className)}
      style={
        {
          '--fp-box': px(BASE),
          '--fp-core': px(3.4),
          '--fp-dot': px(2),
        } as CSSProperties
      }
    >
      <span className="forge-particles-core" />
      {ORBITS.map((o) => (
        <span
          key={o.a}
          className="forge-particles-orbit"
          data-dir={o.dir}
          style={
            {
              '--fp-a': `${o.a}deg`,
              '--fp-r': px(o.r),
              '--fp-t': `${o.t}s`,
              '--fp-tw': `${o.tw}s`,
              '--fp-d': `${o.d}s`,
            } as CSSProperties
          }
        />
      ))}
    </span>
  );
}
