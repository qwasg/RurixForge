/**
 * F7 wave.4 流式 caret(参考 .stream-caret:消息末 2×14 accent 竖条,1s 闪烁)。
 * 动画在 theme.css(@keyframes forge-caret-blink;prefers-reduced-motion 禁闪)。
 */
export default function StreamCaret() {
  return (
    <span
      data-testid="stream-caret"
      className="forge-stream-caret mt-1 inline-block h-[14px] w-[2px] shrink-0 bg-acc"
    />
  );
}
