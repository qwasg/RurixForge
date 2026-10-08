import { useEffect, useState, type FormEvent } from 'react';
import { Navigate, useLocation, useNavigate, type Location } from 'react-router-dom';
import { Button } from '@/components/ui/button';
import { Field, Input } from '@/components/ui/form';
import { FormError } from '@/components/ui/misc';
import { errorMessage } from '@/lib/api/client';
import { useAuth } from '@/lib/auth';
import { DEFAULT_SITE_NAME } from '@/lib/settings';

function targetFrom(state: unknown): string {
  const from = (state as { from?: Location } | null)?.from;
  if (!from || from.pathname === '/login') return '/';
  return `${from.pathname}${from.search ?? ''}${from.hash ?? ''}`;
}

export function LoginPage() {
  const { status, login, authApi } = useAuth();
  const navigate = useNavigate();
  const location = useLocation();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [siteName, setSiteName] = useState(DEFAULT_SITE_NAME);

  useEffect(() => {
    let alive = true;
    authApi
      .config()
      .then((cfg) => {
        if (alive && cfg?.siteName) setSiteName(cfg.siteName);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [authApi]);

  const target = targetFrom(location.state);
  if (status === 'authenticated') return <Navigate to={target} replace />;

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (!email.trim() || !password) {
      setError('请输入邮箱和密码');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await login(email, password);
      navigate(target, { replace: true });
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };

  return (
    <div className="flex min-h-full items-center justify-center bg-muted/40 p-4">
      <div className="w-full max-w-sm">
        <div className="mb-6 flex flex-col items-center gap-2 text-center">
          <svg viewBox="0 0 32 32" className="size-10" aria-hidden>
            <rect width="32" height="32" rx="7" className="fill-foreground" />
            <path d="M10 9h12v3.2h-8.4v3.4h7.2v3.2h-7.2V23H10z" className="fill-background" />
          </svg>
          <h1 className="text-lg font-semibold">{siteName}</h1>
          <p className="text-xs text-muted-foreground">管理后台 · 仅限管理员账号登录</p>
        </div>
        <form onSubmit={onSubmit} className="flex flex-col gap-3 rounded-lg border bg-card p-5 shadow-sm" noValidate>
          <Field label="邮箱">
            <Input
              type="email"
              autoComplete="username"
              autoFocus
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="admin@example.com"
            />
          </Field>
          <Field label="密码">
            <Input
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </Field>
          <FormError>{error}</FormError>
          <Button type="submit" variant="primary" loading={busy} className="mt-1 w-full">
            登录
          </Button>
        </form>
      </div>
    </div>
  );
}
