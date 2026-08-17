import { ChevronDown, Monitor } from 'lucide-react';
import Composer from '../components/Composer';
import { useAppStore } from '../lib/store';

/**
 * Home 主区(布局视频 00:00):白底,垂直水平居中一栏。
 * 顶部位置/环境两枚文字钮 → Composer 大卡 → Plan New Idea / Multitask 胶囊,
 * 页面底部正中为 /review 提示。
 */
export default function HomeView() {
  const openAgent = useAppStore((s) => s.openAgent);

  return (
    <div className="relative h-full bg-white">
      <div className="flex h-full flex-col items-center justify-center">
        <div className="w-full max-w-[640px] px-6">
          {/* 位置 / 环境选择(-ml-1.5 让文字与卡片左缘对齐) */}
          <div className="-ml-1.5 mb-3 flex items-center gap-1">
            <button
              type="button"
              className="flex items-center gap-1 rounded-md px-1.5 py-0.5 text-sm text-ink-soft transition-colors hover:bg-panel-hover"
            >
              <span>{'D:\\游戏引擎'}</span>
              <ChevronDown className="h-3.5 w-3.5 text-muted" />
            </button>
            <button
              type="button"
              className="flex items-center gap-1 rounded-md px-1.5 py-0.5 text-sm text-ink-soft transition-colors hover:bg-panel-hover"
            >
              <Monitor className="h-3.5 w-3.5 text-muted" />
              <span>This PC</span>
              <ChevronDown className="h-3.5 w-3.5 text-muted" />
            </button>
          </div>

          <Composer autoFocus onSend={() => openAgent(null)} />

          {/* 模式入口胶囊 */}
          <div className="mt-3 flex items-center gap-2">
            <button
              type="button"
              className="flex items-center gap-1.5 rounded-full border border-line bg-white px-3 py-1 text-xs text-ink-soft transition-colors hover:bg-panel-hover"
            >
              Plan New Idea
              <span className="text-muted-faint">⇧Tab</span>
            </button>
            <button
              type="button"
              className="rounded-full border border-line bg-white px-3 py-1 text-xs text-ink-soft transition-colors hover:bg-panel-hover"
            >
              Multitask
            </button>
          </div>
        </div>
      </div>

      {/* 底部正中提示 */}
      <div className="pointer-events-none absolute inset-x-0 bottom-10 flex justify-center">
        <p className="text-2xs text-muted-faint">
          Use{' '}
          <span className="rounded bg-panel-active px-1 py-px text-muted">/review</span>{' '}
          for an agentic code review of your changes
        </p>
      </div>
    </div>
  );
}
