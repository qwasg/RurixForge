import type { ImgHTMLAttributes } from 'react';
import { cn } from '@/lib/cn';

export default function ForgeLogo({
  className,
  ...props
}: Omit<ImgHTMLAttributes<HTMLImageElement>, 'src' | 'alt'>) {
  return (
    <img
      {...props}
      src={`${import.meta.env.BASE_URL}logo-transparent.png`}
      alt="RurixForge"
      draggable={false}
      className={cn('forge-logo shrink-0 select-none object-contain', className)}
    />
  );
}
