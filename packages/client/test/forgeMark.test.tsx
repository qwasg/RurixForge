import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import ForgeMark, { FORGE_MARK_PATH } from '@/components/ForgeMark';

/** D-046 品牌标:logo 折带字形矢量版(纯装饰默认隐藏;folded 折面裁在轮廓内;动效走 CSS 类)。 */

afterEach(() => cleanup());

describe('<ForgeMark />', () => {
  it('默认纯装饰:aria-hidden,单路径 evenodd 轮廓 + 内孔', () => {
    const { container } = render(<ForgeMark data-testid="mark" />);
    const svg = screen.getByTestId('mark');
    expect(svg).toHaveAttribute('aria-hidden', 'true');
    expect(svg).not.toHaveAttribute('role');
    expect(svg).toHaveAttribute('data-variant', 'flat');
    const paths = container.querySelectorAll('path');
    expect(paths).toHaveLength(1);
    expect(paths[0]).toHaveAttribute('d', FORGE_MARK_PATH);
    expect(paths[0]).toHaveAttribute('fill-rule', 'evenodd');
    // 外轮廓 + 内孔两段子路径
    expect(FORGE_MARK_PATH.match(/M/g)).toHaveLength(2);
  });

  it('传 title 即为有语义的图', () => {
    render(<ForgeMark title="RurixForge" data-testid="mark" />);
    const svg = screen.getByRole('img', { name: 'RurixForge' });
    expect(svg).not.toHaveAttribute('aria-hidden');
    expect(svg.querySelector('title')).toHaveTextContent('RurixForge');
  });

  it('size 按标识比例给宽高;reveal / busy 挂对应动效类', () => {
    render(<ForgeMark size={22} reveal busy className="text-fg" data-testid="mark" />);
    const svg = screen.getByTestId('mark');
    expect(svg).toHaveAttribute('height', '22');
    expect(svg).toHaveAttribute('width', '25.15');
    expect(svg).toHaveClass('forge-mark-reveal', 'forge-mark-shimmer', 'text-fg');
  });

  it('folded:四个折面裁在轮廓内,多实例 clipPath id 不冲突', () => {
    const { container } = render(
      <>
        <ForgeMark variant="folded" />
        <ForgeMark variant="folded" />
      </>,
    );
    const clips = [...container.querySelectorAll('clipPath')].map((c) => c.id);
    expect(clips).toHaveLength(2);
    expect(new Set(clips).size).toBe(2);
    const groups = container.querySelectorAll('g[clip-path]');
    expect(groups).toHaveLength(2);
    expect(groups[0].querySelectorAll('path')).toHaveLength(4);
    expect(groups[0]).toHaveAttribute('clip-path', `url(#${clips[0]})`);
  });
});
