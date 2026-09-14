/**
 * 对话面呈现变体(2026-08-24 用户拍板「workbench 空态 → 像 Codex 一样全屏输入」):
 * - column:三栏壳里的 360px 对话列(原样);
 * - home:workbench 没有 tab 时对话接管整屏,内容收在 HOME_COL_MAX 居中列;
 * - mini:缩小态,对话脱离三栏流,收成贴主区左下角的浮窗(2026-08-25 用户拍板)。
 * MessageList / Composer / SubagentOverlay 共用同一枚举,避免各处自造字符串。
 * mini 的正文形态与 column 一致(宽度同为 360),各面只需按 home 分叉。
 */
export type ChatVariant = 'column' | 'home' | 'mini';

/** 缩小态浮窗尺寸与贴边留白(px)。 */
export const MINI_CHAT = { w: 360, h: 440, gap: 12 } as const;

/**
 * 把浮窗落点夹进主体区(2026-08-25 用户拍板「缩小窗口要能随意移动」):
 * 整窗必须留在主体区内,免得拖出边界后抓不回来;主体区比浮窗还小时一律贴左上。
 */
export function clampMiniPos(
  x: number,
  y: number,
  bodyW: number,
  bodyH: number,
): { x: number; y: number } {
  const maxX = Math.max(0, bodyW - MINI_CHAT.w);
  const maxY = Math.max(0, bodyH - MINI_CHAT.h);
  return {
    x: Math.min(maxX, Math.max(0, Math.round(x))),
    y: Math.min(maxY, Math.max(0, Math.round(y))),
  };
}

/** 全屏对话主页的正文列宽上限(px)。 */
export const HOME_COL_MAX = 768;

/** 全屏主页输入壳底高:多行盒姿态起步(参考 Codex 首屏大输入框),非胶囊。 */
export const HOME_INPUT_MIN = 72;
