import { FileJson, KeyRound, Link2 } from 'lucide-react';
import { useState } from 'react';
import { Dialog, DialogContent } from '@/components/ui/dialog';
import { Notice } from '@/components/ui/misc';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { COMPLIANCE_HINT } from '@/lib/accounts';
import type { Account, Group } from '@/lib/api/types';
import { ApiKeyTab } from './ApiKeyTab';
import { AuthJsonTab } from './AuthJsonTab';
import { OAuthTab } from './OAuthTab';

export type AddAccountTab = 'authjson' | 'oauth' | 'apikey';

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  groups: Group[];
  accounts: Account[];
  /** 有账号新建/更新后调用（刷新列表）。 */
  onChanged: () => void;
  initialTab?: AddAccountTab;
}

export function AddAccountDialog({ open, onOpenChange, ...rest }: Props) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {open ? <AddAccountBody {...rest} onClose={() => onOpenChange(false)} /> : null}
    </Dialog>
  );
}

function AddAccountBody({
  groups,
  accounts,
  onChanged,
  initialTab = 'authjson',
  onClose,
}: Omit<Props, 'open' | 'onOpenChange'> & { onClose: () => void }) {
  const [tab, setTab] = useState<AddAccountTab>(initialTab);
  return (
    <DialogContent title="添加账号" description="接入 Codex 订阅账号（auth.json / OAuth）或 API Key 账号" size="lg">
      <Tabs value={tab} onValueChange={(v) => setTab(v as AddAccountTab)}>
        <TabsList>
          <TabsTrigger value="authjson">
            <FileJson />
            导入 auth.json
          </TabsTrigger>
          <TabsTrigger value="oauth">
            <Link2 />
            OAuth 授权
          </TabsTrigger>
          <TabsTrigger value="apikey">
            <KeyRound />
            API Key
          </TabsTrigger>
        </TabsList>
        {/* 三个面板常驻（切换标签不丢已填内容与 OAuth 会话），非活动面板用 hidden 隐藏。 */}
        <TabsContent value="authjson" forceMount hidden={tab !== 'authjson'}>
          <Notice tone="warning" className="mb-4">
            {COMPLIANCE_HINT}
          </Notice>
          <AuthJsonTab groups={groups} onImported={onChanged} onDone={onClose} />
        </TabsContent>
        <TabsContent value="oauth" forceMount hidden={tab !== 'oauth'}>
          <Notice tone="warning" className="mb-4">
            {COMPLIANCE_HINT}
          </Notice>
          <OAuthTab groups={groups} accounts={accounts} onAdded={onChanged} onDone={onClose} />
        </TabsContent>
        <TabsContent value="apikey" forceMount hidden={tab !== 'apikey'}>
          <ApiKeyTab groups={groups} onAdded={onChanged} onDone={onClose} />
        </TabsContent>
      </Tabs>
    </DialogContent>
  );
}
