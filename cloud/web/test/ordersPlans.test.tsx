import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { AdminOrder, Plan } from '@/lib/api/types';
import { ToastProvider } from '@/lib/toast';
import { OrdersPage } from '@/pages/OrdersPage';
import { PlansPage } from '@/pages/PlansPage';
import { json, mockFetch } from './helpers';

const pendingOrder: AdminOrder = {
  id: 42,
  kind: 'subscription',
  provider: 'fakepay',
  status: 'pending',
  amountMicros: 8_000_000,
  listPriceMicros: 200_000_000,
  creditMicros: 192_000_000,
  planId: 4,
  planName: 'Ultra',
  tier: 'ultra',
  interval: 'month',
  mode: 'upgrade',
  replacesSubscriptionId: 7,
  subscriptionId: null,
  payUrl: 'https://pay.example.test/o/42',
  note: '线上支付',
  createdAt: '2026-09-27T08:00:00Z',
  paidAt: null,
  userId: 5,
  userEmail: 'buyer@example.com',
};

function pathOf(url: string): string {
  return new URL(url, 'http://localhost').pathname;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('OrdersPage', () => {
  it('lists orders and marks a pending order as paid with a trimmed note', async () => {
    let paid = false;
    const { fn, calls } = mockFetch((c) => {
      const path = pathOf(c.url);
      if (c.method === 'GET' && path === '/api/admin/orders') {
        const item = paid ? { ...pendingOrder, status: 'paid', paidAt: '2026-09-27T08:05:00Z' } : pendingOrder;
        return json(200, { items: [item], total: 1 });
      }
      if (c.method === 'POST' && path === '/api/admin/orders/42/mark-paid') {
        paid = true;
        return json(200, { ...pendingOrder, status: 'paid' });
      }
      return undefined;
    });
    vi.stubGlobal('fetch', fn);
    render(
      <ToastProvider>
        <OrdersPage />
      </ToastProvider>,
    );

    const table = await screen.findByRole('table');
    expect(await within(table).findByText('buyer@example.com')).toBeInTheDocument();
    expect(within(table).getByText('Ultra · 按月 · 升级')).toBeInTheDocument();
    expect(within(table).getByText('待支付')).toBeInTheDocument();
    expect(within(table).getByText('fakepay')).toBeInTheDocument();

    fireEvent.click(within(table).getByRole('button', { name: /标记已支付/ }));
    const dialog = await screen.findByRole('dialog');
    fireEvent.change(within(dialog).getByLabelText('备注'), { target: { value: '  转账流水 A1  ' } });
    fireEvent.click(within(dialog).getByRole('button', { name: '确认已支付' }));

    await waitFor(() => expect(calls.some((c) => pathOf(c.url).endsWith('/mark-paid'))).toBe(true));
    expect(calls.find((c) => pathOf(c.url).endsWith('/mark-paid'))?.body).toEqual({ note: '转账流水 A1' });
    expect(await within(table).findByText('已支付')).toBeInTheDocument();
    expect(within(table).queryByRole('button', { name: /标记已支付/ })).not.toBeInTheDocument();
  });
});

describe('PlansPage', () => {
  it('shows tiers as a ladder with the yearly discount and packs separately', async () => {
    const plans: Plan[] = [
      {
        id: 2,
        name: 'Pro',
        description: '',
        priceMicros: 20_000_000,
        periodDays: 30,
        quotaMicros: 20_000_000,
        dailyLimitMicros: 0,
        groupId: null,
        enabled: true,
        tier: 'pro',
        tierRank: 10,
        tagline: '适合开始使用 Agent 的开发者',
        features: ['套餐内额度每月重置'],
        priceYearlyMicros: 192_000_000,
        forgeQuotaMicros: 60_000_000,
        highlight: true,
        subscriberCount: 3,
      },
      {
        id: 9,
        name: 'Starter 额度包',
        description: '一次性额度',
        priceMicros: 5_000_000,
        periodDays: 90,
        quotaMicros: 5_000_000,
        dailyLimitMicros: 0,
        groupId: null,
        enabled: false,
        tier: '',
        tierRank: 0,
        tagline: '',
        features: [],
        priceYearlyMicros: 0,
        forgeQuotaMicros: 0,
        highlight: false,
        subscriberCount: 0,
      },
    ];
    const { fn } = mockFetch((c) => {
      const path = pathOf(c.url);
      if (path === '/api/admin/plans') return json(200, { items: plans });
      if (path === '/api/admin/groups') return json(200, { items: [] });
      return undefined;
    });
    vi.stubGlobal('fetch', fn);
    render(
      <ToastProvider>
        <PlansPage />
      </ToastProvider>,
    );

    const [tierTable, packTable] = await screen.findAllByRole('table');
    expect(await within(tierTable).findByText('Pro')).toBeInTheDocument();
    expect(within(tierTable).getByText('pro')).toBeInTheDocument();
    expect(within(tierTable).getByText('推荐')).toBeInTheDocument();
    expect(within(tierTable).getByText('8 折')).toBeInTheDocument();
    expect(within(tierTable).queryByText('Starter 额度包')).not.toBeInTheDocument();
    expect(within(packTable).getByText('Starter 额度包')).toBeInTheDocument();
    expect(within(packTable).getByText('停用')).toBeInTheDocument();
  });
});
