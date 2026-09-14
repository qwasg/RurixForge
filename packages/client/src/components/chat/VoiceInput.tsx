import { Mic, MicOff } from 'lucide-react';
import { cn } from '@/lib/cn';
import { formatElapsed } from '@/lib/speechInput';

/**
 * Composer 工具行语音输入钮(2026-08-25:落在上下文计量环右侧,与其同 22px 胶囊规格)。
 * 三态:
 * - 空闲 = 灰麦克风,点开始听写,识别文本实时接在草稿后;
 * - 聆听 = danger_bg 胶囊 + 呼吸麦克风 + 已录时长(font-code),再点或 Esc 停;
 * - 不支持 = 划线麦克风 + 禁用,title 如实说明原因(见 lib/speechInput.ts 头注的能力边界)。
 */

const UNSUPPORTED_TITLE = '当前环境不支持语音输入（需 Chrome / Edge 的 Web Speech 识别服务）';

export function VoiceInputButton({
  supported,
  listening,
  seconds,
  onToggle,
}: {
  supported: boolean;
  listening: boolean;
  seconds: number;
  onToggle: () => void;
}) {
  const label = !supported ? '语音输入不可用' : listening ? '停止语音输入' : '语音输入';
  const title = !supported
    ? UNSUPPORTED_TITLE
    : listening
      ? `聆听中 ${formatElapsed(seconds)}，点击停止（Esc 亦可）`
      : '语音输入：边说边写入输入框';

  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={listening}
      data-testid="composer-voice"
      title={title}
      disabled={!supported}
      onClick={onToggle}
      className={cn(
        'flex h-[22px] items-center gap-1 rounded-md px-1.5 text-[11px]',
        listening ? 'bg-danger-bg text-danger' : 'text-fg-2 hover:bg-shell-hover',
        !supported && 'cursor-not-allowed opacity-40 hover:bg-transparent',
      )}
    >
      {supported ? (
        <Mic size={11} className={listening ? 'animate-pulse text-danger' : 'text-fg-3'} />
      ) : (
        <MicOff size={11} className="text-fg-4" />
      )}
      {listening && (
        <span className="font-code text-[10.5px]" data-testid="composer-voice-elapsed">
          {formatElapsed(seconds)}
        </span>
      )}
    </button>
  );
}
