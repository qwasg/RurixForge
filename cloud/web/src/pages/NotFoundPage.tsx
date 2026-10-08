import { Link } from 'react-router-dom';
import { buttonVariants } from '@/components/ui/button';

export function NotFoundPage() {
  return (
    <div className="flex flex-col items-center justify-center gap-3 py-24 text-center">
      <p className="text-4xl font-semibold text-muted-foreground">404</p>
      <p className="text-sm text-muted-foreground">页面不存在</p>
      <Link to="/" className={buttonVariants({ size: 'sm' })}>
        返回仪表盘
      </Link>
    </div>
  );
}
