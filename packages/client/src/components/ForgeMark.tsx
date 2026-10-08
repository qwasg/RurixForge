import { useId, type SVGProps } from 'react';
import { cn } from '@/lib/cn';

/**
 * 品牌标(D-046):public/logo.png 折带字形的矢量版。坐标直接取自 1254px 原图轮廓追踪
 * (外轮廓 22 点 + 三角内孔,evenodd),viewBox 即原图标识区,与 PNG 可逐像素对照。
 * fill = currentColor,亮暗主题随文字色反转;ForgeLogo(PNG 图块)仍用于标题栏/关于/助手头像。
 *
 * folded:叠四个折面(顶横带受光面、环顶内折面、右上臂、右下臂),填 var(--bg) 低透明渐变——
 * 亮色下提亮、暗色下压暗,同一条规则两主题通用;交叉处保持底色,读作「从主斜带下方穿过」。
 */

const VIEW_BOX = '205 262 846 740';
const ASPECT = 846 / 740;

export const FORGE_MARK_PATH =
  'M356 271H578L706 433L838 301H1024L1029 306L1023 322L819 584L1041 892L1043 904L1038 909H856L794 831L660 993L407 990L241 760L360 569L576 567L585 558L488 435H218L213 422Z' +
  'M499 694L692 695L703 708L572 863L455 698Z';

/** 折面:多边形 + 渐变方向(objectBoundingBox 两端点)+ 起止不透明度。 */
const FOLD_FACES: Array<{ d: string; from: [number, number]; to: [number, number]; opacity: [number, number] }> = [
  // 顶横带受光面:左上亮,收到折痕
  { d: 'M356 271H578L488 435H218L213 422Z', from: [0, 0], to: [1, 1], opacity: [0.22, 0] },
  // 环顶内折面:左亮右暗(右端压在主斜带下)
  { d: 'M382 569L585 558L703 708L460 697Z', from: [0, 0], to: [1, 0], opacity: [0.28, 0] },
  // 右上臂:右上亮,到交叉处归零
  { d: 'M706 433L838 301H1024L1029 306L1023 322L819 584Z', from: [1, 0], to: [0, 1], opacity: [0.2, 0] },
  // 右下臂:左下亮,到主斜带处归零
  { d: 'M703 708L794 831L660 993L572 863Z', from: [0, 1], to: [1, 0], opacity: [0.2, 0] },
];

export interface ForgeMarkProps extends Omit<SVGProps<SVGSVGElement>, 'children' | 'ref' | 'title'> {
  /** flat = 纯轮廓(默认,小尺寸);folded = 加折面(大尺寸主视觉) */
  variant?: 'flat' | 'folded';
  /** 入场:沿主斜带方向擦出(只播一次) */
  reveal?: boolean;
  /** 忙碌:亮带循环扫过 */
  busy?: boolean;
  /** 像素高度(宽按比例);不传则由 className 定尺寸 */
  size?: number;
  /** 传了即为有语义的图(role=img);默认纯装饰 aria-hidden */
  title?: string;
}

export default function ForgeMark({
  variant = 'flat',
  reveal = false,
  busy = false,
  size,
  title,
  className,
  style,
  ...props
}: ForgeMarkProps) {
  const uid = useId().replace(/:/g, '');
  const clipId = `forge-mark-clip-${uid}`;
  const a11y = title ? { role: 'img', 'aria-label': title } : { 'aria-hidden': true as const };
  const dims = size ? { height: size, width: Math.round(size * ASPECT * 100) / 100 } : {};
  return (
    <svg
      viewBox={VIEW_BOX}
      fill="currentColor"
      focusable="false"
      data-variant={variant}
      {...dims}
      {...a11y}
      {...props}
      // 只给高或只给宽(className)时另一维按标识比例推出
      style={{ aspectRatio: '846 / 740', ...style }}
      className={cn('shrink-0', reveal && 'forge-mark-reveal', busy && 'forge-mark-shimmer', className)}
    >
      {title && <title>{title}</title>}
      <path d={FORGE_MARK_PATH} fillRule="evenodd" />
      {variant === 'folded' && (
        <>
          <defs>
            <clipPath id={clipId}>
              <path d={FORGE_MARK_PATH} clipRule="evenodd" />
            </clipPath>
            {FOLD_FACES.map((f, i) => (
              <linearGradient key={f.d} id={`${clipId}-f${i}`} x1={f.from[0]} y1={f.from[1]} x2={f.to[0]} y2={f.to[1]}>
                <stop offset="0" stopOpacity={f.opacity[0]} style={{ stopColor: 'var(--bg)' }} />
                <stop offset="1" stopOpacity={f.opacity[1]} style={{ stopColor: 'var(--bg)' }} />
              </linearGradient>
            ))}
          </defs>
          <g clipPath={`url(#${clipId})`}>
            {FOLD_FACES.map((f, i) => (
              <path key={f.d} d={f.d} fill={`url(#${clipId}-f${i})`} />
            ))}
          </g>
        </>
      )}
    </svg>
  );
}
