import { useEffect, useState } from 'react';
import { avatarUrl, type CloudUser } from '@/lib/accountApi';

/** 云账号头像:有图显示图(加载失败退回首字母),否则昵称/邮箱首字(D-046 折面底,呼应 logo 折带)。 */
export default function CloudAvatar({ user, size = 28 }: { user: CloudUser; size?: number }) {
  const [broken, setBroken] = useState(false);
  useEffect(() => setBroken(false), [user.avatarVersion]);
  const initial = ((user.nickname || user.email).match(/[\p{L}\p{N}]/u)?.[0] ?? '·').toUpperCase();
  const style = { width: size, height: size };
  if (user.hasAvatar && !broken) {
    return (
      <img
        data-testid="cloud-avatar-img"
        src={avatarUrl(user.avatarVersion)}
        alt=""
        onError={() => setBroken(true)}
        style={style}
        className="shrink-0 rounded-full object-cover ring-1 ring-edge"
      />
    );
  }
  return (
    <span
      data-testid="cloud-avatar-initial"
      style={{ ...style, fontSize: Math.round(size * 0.43) }}
      className="forge-fold-avatar flex shrink-0 select-none items-center justify-center rounded-full font-medium"
    >
      {initial}
    </span>
  );
}
