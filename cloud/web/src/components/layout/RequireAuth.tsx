import type { ReactNode } from 'react';
import { Navigate, useLocation } from 'react-router-dom';
import { LoadingBlock } from '@/components/ui/misc';
import { useAuth } from '@/lib/auth';

/** 未登录（含会话过期）→ 跳登录页，并记住来源页供登录后返回。 */
export function RequireAuth({ children }: { children: ReactNode }) {
  const { status } = useAuth();
  const location = useLocation();
  if (status === 'loading') {
    return (
      <div className="flex h-full items-center justify-center">
        <LoadingBlock text="正在恢复登录状态…" />
      </div>
    );
  }
  if (status !== 'authenticated') {
    return <Navigate to="/login" replace state={{ from: location }} />;
  }
  return <>{children}</>;
}
