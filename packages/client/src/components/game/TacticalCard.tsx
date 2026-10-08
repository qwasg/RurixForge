import { useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type DragEvent, type PointerEvent } from 'react';
import { Check, Coins, Cpu, RotateCcw, Zap } from 'lucide-react';
import './TacticalCard.css';

export const TACTICAL_CARD_MIME = 'application/x-code-sentinels-card';
export const TACTICAL_CARD_FRAME = '/games/code-sentinels/ui-v3/operator-card-frame.svg';

export type TacticalCardProps = {
  variant: 'operator' | 'hardware';
  id: number;
  name: string;
  subtitle: string;
  code: string;
  cost: number;
  energyCost?: number;
  production?: number;
  artwork: string;
  photo?: string;
  accent: string;
  selected: boolean;
  /** Funds only; technology requirements are reported separately through lockedReason. */
  affordable: boolean;
  lockedReason?: string;
  hotkey: string;
  details: string[];
  onChoose(): void;
  onDragState?(dragging: boolean): void;
};

const amount = (value: number) => value.toLocaleString('en-US', { maximumFractionDigits: 1 });

/** A presentation/drag-source component. Choosing or dragging a card never purchases anything itself. */
export default function TacticalCard({ variant, id, name, subtitle, code, cost, energyCost, production,
  artwork, photo, accent, selected, affordable, lockedReason, hotkey, details, onChoose, onDragState }: TacticalCardProps) {
  const [flipped, setFlipped] = useState(false);
  const [dragging, setDragging] = useState(false);
  const card = useRef<HTMLElement>(null);
  const dragActive = useRef(false);
  const dragCallback = useRef(onDragState);
  const descriptionId = useId();
  const backId = useId();
  useLayoutEffect(() => { dragCallback.current = onDragState; }, [onDragState]);
  useEffect(() => { setFlipped(false); }, [variant, id]);
  useEffect(() => () => { if (dragActive.current) dragCallback.current?.(false); }, []);

  const resetTilt = () => {
    card.current?.style.setProperty('--tc-tilt-x', '0deg');
    card.current?.style.setProperty('--tc-tilt-y', '0deg');
  };
  const tilt = (event: PointerEvent<HTMLElement>) => {
    if (event.pointerType === 'touch' || dragActive.current) return;
    const bounds = card.current?.getBoundingClientRect();
    if (!bounds?.width || !bounds.height) return;
    const x = Math.max(-1, Math.min(1, (event.clientX - bounds.left) / bounds.width * 2 - 1));
    const y = Math.max(-1, Math.min(1, (event.clientY - bounds.top) / bounds.height * 2 - 1));
    card.current?.style.setProperty('--tc-tilt-x', `${(-y * 4).toFixed(2)}deg`);
    card.current?.style.setProperty('--tc-tilt-y', `${(x * 5).toFixed(2)}deg`);
  };
  const dragStart = (event: DragEvent<HTMLButtonElement>) => {
    if (!event.dataTransfer) return;
    event.dataTransfer.setData(TACTICAL_CARD_MIME, JSON.stringify({ kind: variant, id }));
    event.dataTransfer.effectAllowed = 'copy';
    dragActive.current = true;
    setDragging(true);
    resetTilt();
    dragCallback.current?.(true);
  };
  const dragEnd = () => {
    if (!dragActive.current) return;
    dragActive.current = false;
    setDragging(false);
    dragCallback.current?.(false);
  };
  const economics = [
    `建设经费 ${amount(cost)}${affordable ? '' : '，当前经费不足，仍可选择查看'}`,
    lockedReason ? `科技尚未解锁：${lockedReason}，仍可选择查看` : '',
    energyCost !== undefined ? `普攻消耗 ${amount(energyCost)} 算力/发` : '',
    production !== undefined ? `游戏产能 ${amount(production)} 算力/秒` : '',
  ].filter(Boolean);
  const style = { '--tc-accent': accent } as CSSProperties;

  return <article ref={card} style={style} onPointerMove={tilt} onPointerLeave={resetTilt}
    className={`tc-card tc-card--${variant}${selected ? ' tc-is-selected' : ''}${flipped ? ' tc-is-flipped' : ''}${dragging ? ' tc-is-dragging' : ''}${affordable ? '' : ' tc-is-unaffordable'}`}
    data-card-id={id} data-card-kind={variant} data-flipped={flipped}>
    <button type="button" className="tc-choose" aria-label={`选择 ${name}`} aria-pressed={selected}
      aria-describedby={descriptionId} draggable onDragStart={dragStart} onDragEnd={dragEnd}
      onClick={() => { if (!dragActive.current) onChoose(); }}>
      <span className="tc-turner">
        <span className="tc-face tc-front" aria-hidden={flipped}>
          <img className="tc-artwork" src={artwork} alt="" draggable={false} decoding="async"/>
          <span className="tc-art-shade"/>
          {variant === 'hardware' && photo && <span className="tc-hardware-photo"><img src={photo} alt={`${name} 官方实物图`} draggable={false} decoding="async"/></span>}
          <img className="tc-frame" src={TACTICAL_CARD_FRAME} alt="" draggable={false}/>
          <span className="tc-topline">
            <kbd className="tc-hotkey">{hotkey}</kbd><span className="tc-subtitle" title={subtitle}>{subtitle}</span>
            <span className="tc-cost" aria-label={`建设经费 ${amount(cost)}${affordable ? '' : '，当前经费不足'}`}><Coins size={11}/>{amount(cost)}</span>
          </span>
          {selected && <span className="tc-selected-mark" aria-hidden="true"><Check size={13}/></span>}
          <span className="tc-front-info"><span className="tc-code">{code}</span><strong className="tc-name">{name}</strong>
            <span className="tc-front-stat">{variant === 'hardware' && production !== undefined
              ? <><Cpu size={11}/><b>+{amount(production)}</b><span>/s</span></>
              : energyCost !== undefined ? <><Zap size={11}/><b>{amount(energyCost)}</b><span>/发</span></> : <span>{subtitle}</span>}</span>
          </span>
        </span>
        <span id={backId} className="tc-face tc-back" aria-hidden={!flipped}>
          <span className="tc-back-grid"/><img className="tc-frame" src={TACTICAL_CARD_FRAME} alt="" draggable={false}/>
          <span className="tc-back-content"><span className="tc-back-code">{code}</span><strong className="tc-back-name">{name}</strong>
            <span className="tc-back-subtitle">{subtitle}</span>
            <span className="tc-stats"><span><span>建设经费</span><b className="tc-back-cost">{amount(cost)}</b></span>
              {energyCost !== undefined && <span><span>普攻耗能</span><b>{amount(energyCost)}/发</b></span>}
              {production !== undefined && <span><span>游戏产能</span><b>+{amount(production)}/s</b></span>}
            </span>
            <span className="tc-details" role="list">{details.map((detail, index) => <span role="listitem" key={`${index}-${detail}`} title={detail}>{detail}</span>)}</span>
          </span>
        </span>
      </span>
    </button>
    <button type="button" className="tc-flip" aria-label={flipped ? `返回 ${name} 正面` : `翻转 ${name} 查看详情`}
      aria-pressed={flipped} aria-controls={backId} title={flipped ? '返回卡牌正面' : '翻面查看详情'}
      onPointerDown={(event) => event.stopPropagation()}
      onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') event.stopPropagation(); }}
      onClick={(event) => { event.stopPropagation(); setFlipped((value) => !value); }}>
      <RotateCcw size={13}/><span>{flipped ? '正面' : '详情'}</span>
    </button>
    <span id={descriptionId} className="tc-sr-only">{economics.join('；')}。{details.join('；')}</span>
  </article>;
}
