import type { CodexInstallStatus } from '@/lib/forgeApi';

const stageLabels = {
  starting: '正在检查安装环境…',
  downloading: '正在下载并安装 Codex 组件…',
  preparing: '正在准备 Computer Use 组件…',
  verifying: '正在验证 Codex 能否启动…',
  finished: 'Codex 安装检查通过',
  failed: 'Codex 安装失败，可重试',
} satisfies Record<NonNullable<CodexInstallStatus['stage']>, string>;

export default function CodexSetupProgress({ install, installing, startingLogin }: {
  install?: CodexInstallStatus;
  installing?: boolean;
  startingLogin?: boolean;
}) {
  const stage = startingLogin ? '正在启动 Codex 并请求授权…'
    : install?.stage ? stageLabels[install.stage]
    : install?.running || installing ? stageLabels.starting : null;
  if (!stage && !install?.log) return null;
  return (
    <div className="min-w-0 px-3 py-2 text-[11px] text-fg-2">
      {stage && <span role="status">{stage}</span>}
      {install?.running && install.elapsedSeconds != null && (
        <span className="ml-2 text-fg-3">已用 {install.elapsedSeconds} 秒</span>
      )}
      {install?.log && (
        <pre aria-label="Codex 安装日志" className="mt-1 max-h-32 overflow-auto whitespace-pre-wrap break-words rounded bg-shell-hover p-2 font-code text-[10px] text-fg-3">
          {install.log.trimEnd()}
        </pre>
      )}
    </div>
  );
}
