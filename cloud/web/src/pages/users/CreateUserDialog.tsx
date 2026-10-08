import { useId, useState, type FormEvent } from 'react';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent } from '@/components/ui/dialog';
import { Field, Input, Select } from '@/components/ui/form';
import { FormError } from '@/components/ui/misc';
import { adminApi } from '@/lib/api/admin';
import { errorMessage } from '@/lib/api/client';
import type { Group, Role } from '@/lib/api/types';
import { unitsToMicros } from '@/lib/format';
import { useSettings } from '@/lib/settings';
import { useToast } from '@/lib/toast';

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  groups: Group[];
  onCreated: () => void;
}

export function CreateUserDialog({ open, onOpenChange, ...rest }: Props) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {open ? <CreateUserForm {...rest} onClose={() => onOpenChange(false)} /> : null}
    </Dialog>
  );
}

function CreateUserForm({ groups, onCreated, onClose }: Omit<Props, 'open' | 'onOpenChange'> & { onClose: () => void }) {
  const formId = useId();
  const toast = useToast();
  const { currency } = useSettings();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [nickname, setNickname] = useState('');
  const [role, setRole] = useState<Role>('user');
  const [groupId, setGroupId] = useState('');
  const [balance, setBalance] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const addr = email.trim();
    if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(addr)) return setError('请输入有效的邮箱地址');
    if (password.length < 8) return setError('密码至少 8 位');
    const balanceMicros = balance.trim() === '' ? 0 : unitsToMicros(balance);
    if (Number.isNaN(balanceMicros) || balanceMicros < 0) return setError('初始余额应为非负数字');
    setError(null);
    setBusy(true);
    try {
      await adminApi.users.create({
        email: addr,
        password,
        nickname: nickname.trim(),
        role,
        groupId: groupId ? Number(groupId) : null,
        balanceMicros,
      });
      toast.success(`已创建用户 ${addr}`);
      onCreated();
      onClose();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogContent
      title="新建用户"
      size="sm"
      footer={
        <>
          <Button onClick={onClose} disabled={busy}>
            取消
          </Button>
          <Button type="submit" form={formId} variant="primary" loading={busy}>
            创建
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={onSubmit} className="flex flex-col gap-3" noValidate>
        <Field label="邮箱" required>
          <Input type="email" value={email} onChange={(e) => setEmail(e.target.value)} autoFocus />
        </Field>
        <Field label="密码" required hint="至少 8 位">
          <Input type="text" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="new-password" />
        </Field>
        <Field label="昵称">
          <Input value={nickname} onChange={(e) => setNickname(e.target.value)} maxLength={32} />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="角色">
            <Select value={role} onChange={(e) => setRole(e.target.value as Role)}>
              <option value="user">用户</option>
              <option value="admin">管理员</option>
            </Select>
          </Field>
          <Field label="分组">
            <Select value={groupId} onChange={(e) => setGroupId(e.target.value)}>
              <option value="">默认分组</option>
              {groups.map((g) => (
                <option key={g.id} value={g.id}>
                  {g.name}
                </option>
              ))}
            </Select>
          </Field>
        </div>
        <Field label={`初始余额（${currency}）`} hint="可留空，默认 0">
          <Input inputMode="decimal" value={balance} onChange={(e) => setBalance(e.target.value)} placeholder="0" />
        </Field>
        <FormError>{error}</FormError>
      </form>
    </DialogContent>
  );
}
