import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import TacticalCard, { TACTICAL_CARD_FRAME, TACTICAL_CARD_MIME, type TacticalCardProps } from '@/components/game/TacticalCard';

const props = (overrides: Partial<TacticalCardProps> = {}): TacticalCardProps => ({
  variant: 'operator', id: 3, name: 'DeepSeek 娘', subtitle: '潮汐控制', code: 'OP-03', cost: 110,
  energyCost: 5, artwork: '/games/code-sentinels/ui-v3/art/operator-deepseek.png', accent: '#6cd1f4',
  selected: false, affordable: true, hotkey: '3', details: ['推理潮汐 · 90 算力', '控制周围敌人'],
  onChoose: vi.fn(), onDragState: vi.fn(), ...overrides,
});
const transfer = () => ({ setData: vi.fn(), effectAllowed: 'none' });
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

describe('TacticalCard presentation and input contract', () => {
  it('uses separate accessible choose/flip buttons and flipping never selects the card', () => {
    const card = props(); const { container } = render(<TacticalCard {...card}/>);
    expect(container.querySelector('button button')).toBeNull();
    expect(screen.getByRole('button', { name: '选择 DeepSeek 娘' })).toHaveAttribute('aria-pressed', 'false');
    fireEvent.click(screen.getByRole('button', { name: '翻转 DeepSeek 娘 查看详情' }));
    expect(card.onChoose).not.toHaveBeenCalled();
    expect(container.querySelector('.tc-card')).toHaveAttribute('data-flipped', 'true');
    expect(screen.getByRole('button', { name: '返回 DeepSeek 娘 正面' })).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(screen.getByRole('button', { name: '返回 DeepSeek 娘 正面' }));
    expect(container.querySelector('.tc-card')).toHaveAttribute('data-flipped', 'false');
    fireEvent.click(screen.getByRole('button', { name: '选择 DeepSeek 娘' }));
    expect(card.onChoose).toHaveBeenCalledOnce();
    expect(container.querySelectorAll(`img[src="${TACTICAL_CARD_FRAME}"]`)).toHaveLength(2);
  });

  it('allows selecting and inspecting an unaffordable card instead of silently disabling it', () => {
    const card = props({ affordable: false, selected: true }); const { container } = render(<TacticalCard {...card}/>);
    const choose = screen.getByRole('button', { name: '选择 DeepSeek 娘' });
    expect(choose).toBeEnabled(); expect(choose).toHaveAttribute('aria-pressed', 'true');
    expect(choose).toHaveAccessibleDescription(/当前经费不足，仍可选择查看/);
    expect(choose).not.toHaveAccessibleDescription(/科技尚未解锁/);
    expect(container.querySelector('.tc-card')).toHaveClass('tc-is-unaffordable');
    fireEvent.click(choose); expect(card.onChoose).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole('button', { name: '翻转 DeepSeek 娘 查看详情' }));
    expect(card.onChoose).toHaveBeenCalledOnce();
  });

  it('describes a technology requirement independently of sufficient funds and keeps inspection available', () => {
    const card = props({ variant: 'hardware', name: 'RTX 5080', affordable: true,
      lockedReason: '需要 T1 科技（当前 T0）' });
    const { container } = render(<TacticalCard {...card}/>);
    const choose = screen.getByRole('button', { name: '选择 RTX 5080' });
    expect(choose).toBeEnabled();
    expect(choose).toHaveAccessibleDescription(/科技尚未解锁：需要 T1 科技（当前 T0），仍可选择查看/);
    expect(choose).not.toHaveAccessibleDescription(/当前经费不足/);
    expect(container.querySelector('.tc-card')).not.toHaveClass('tc-is-unaffordable');
    expect(container.querySelector('.tc-cost')).toHaveAttribute('aria-label', '建设经费 110');
    fireEvent.click(choose);
    expect(card.onChoose).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole('button', { name: '翻转 RTX 5080 查看详情' }));
    expect(container.querySelector('.tc-card')).toHaveAttribute('data-flipped', 'true');
    expect(card.onChoose).toHaveBeenCalledOnce();
  });

  it('reports both missing funds and a technology requirement when both apply', () => {
    render(<TacticalCard {...props({ affordable: false, lockedReason: '需要 T2 科技' })}/>);
    const choose = screen.getByRole('button', { name: '选择 DeepSeek 娘' });
    expect(choose).toHaveAccessibleDescription(/当前经费不足，仍可选择查看/);
    expect(choose).toHaveAccessibleDescription(/科技尚未解锁：需要 T2 科技，仍可选择查看/);
    expect(choose).toBeEnabled();
  });

  it.each(['operator', 'hardware'] as const)('writes only the %s card identity to the required drag MIME', (variant) => {
    const card = props({ variant, id: 7 }); render(<TacticalCard {...card}/>);
    const dataTransfer = transfer(), choose = screen.getByRole('button', { name: '选择 DeepSeek 娘' });
    fireEvent.dragStart(choose, { dataTransfer });
    expect(dataTransfer.setData.mock.calls).toEqual([[TACTICAL_CARD_MIME, JSON.stringify({ kind: variant, id: 7 })]]);
    expect(dataTransfer.effectAllowed).toBe('copy');
    expect(card.onDragState).toHaveBeenNthCalledWith(1, true);
    expect(card.onChoose).not.toHaveBeenCalled();
    fireEvent.dragEnd(choose, { dataTransfer });
    expect(card.onDragState).toHaveBeenNthCalledWith(2, false);
    expect(card.onChoose).not.toHaveBeenCalled();
  });

  it('clears a drag if its parent hides/unmounts the hand before dragend arrives', () => {
    const card = props(); const { unmount } = render(<TacticalCard {...card}/>);
    fireEvent.dragStart(screen.getByRole('button', { name: '选择 DeepSeek 娘' }), { dataTransfer: transfer() });
    unmount();
    expect(card.onDragState).toHaveBeenLastCalledWith(false);
    expect(card.onDragState).toHaveBeenCalledTimes(2);
  });

  it('retains the real hardware photo, precise production and shared vertical frame', () => {
    const card = props({ variant: 'hardware', id: 1, name: 'RTX 5060', production: 18.6,
      energyCost: undefined, photo: '/games/code-sentinels/gpus/rtx-5060-msi-ventus2x.png',
      artwork: '/games/code-sentinels/ui-v3/art/hardware-consumer.png' });
    const { container } = render(<TacticalCard {...card}/>);
    const photo = screen.getByAltText('RTX 5060 官方实物图');
    expect(photo).toHaveAttribute('src', card.photo);
    expect(photo).toHaveAttribute('draggable', 'false');
    expect(container.querySelector('.tc-front-stat')).toHaveTextContent('+18.6/s');
    expect(screen.getByRole('button', { name: '选择 RTX 5060' })).toHaveAccessibleDescription(/18.6 算力\/秒/);
  });

  it('keeps Enter/Space on the flip control from reaching global purchase/pause shortcuts', () => {
    const gameKey = vi.fn(); window.addEventListener('keydown', gameKey);
    const card = props(); render(<TacticalCard {...card}/>);
    const flip = screen.getByRole('button', { name: '翻转 DeepSeek 娘 查看详情' });
    fireEvent.keyDown(flip, { key: 'Enter' }); fireEvent.keyDown(flip, { key: ' ' });
    expect(gameKey).not.toHaveBeenCalled(); expect(card.onChoose).not.toHaveBeenCalled();
    window.removeEventListener('keydown', gameKey);
  });
});
