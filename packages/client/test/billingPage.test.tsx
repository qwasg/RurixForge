import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import BillingPage from '@/components/settings/BillingPage';
import AccountPage from '@/components/settings/AccountPage';
import UsageDashboard, { cacheHitRate, fillDays, recentUsageRange } from '@/components/settings/UsageDashboard';
import { useAccountStore } from '@/lib/accountStore';
import type { UsageItem, UsageSummary } from '@/lib/accountApi';
import { ACTIVITY_DAYS, activityLevel, buildUsageActivity } from '@/lib/usageActivity';
import { jsonResponse, makeAccountStatus } from './accountTestHelpers';

const initial = useAccountStore.getState();
const cycle = { start: '2026-10-01T00:00:00Z', end: '2026-11-01T00:00:00Z' };
const user = { id: 7, email: 'member@test.com', nickname: 'Member', role: 'user', status: 'active', hasAvatar: false, avatarVersion: 0, balanceMicros: 0 };
const item: UsageItem = {
  id: 31, requestId: 'call-31', model: 'research-model', endpoint: 'responses', stream: true,
  inputTokens: 1000, outputTokens: 64, cacheReadTokens: 850, cacheWriteTokens: 12,
  costMicros: 42, status: 'ok', latencyMs: 1240, createdAt: '2026-10-07T07:00:00Z',
};
const summary: UsageSummary = {
  requests: 51, inputTokens: 125000, outputTokens: 25000, cacheReadTokens: 75000,
  cacheWriteTokens: 1000, costMicros: 300000,
};
const page = { items: [item], total: 51, summary };
const membership = {
  tier: { name: 'Hobby', tier: 'hobby' }, cycle, currency: 'USD',
  pools: { api: { includedMicros: 2000000, usedMicros: 500000, remainingMicros: 1500000 }, forge: { includedMicros: 1000000, remainingMicros: 1000000 } },
};
type Handler = (path: string, query: URLSearchParams) => Response | Promise<Response> | undefined;
let unexpected: string[] = [];

function api(handler?: Handler) {
  const requests: Array<{ path: string; method: string; query: URLSearchParams }> = [];
  const fetch = vi.fn(async (input: unknown, init?: RequestInit) => {
    const url = new URL(String(input), 'http://localhost');
    const method = init?.method ?? 'GET';
    requests.push({ path: url.pathname, method, query: url.searchParams });
    const custom = handler?.(url.pathname, url.searchParams);
    if (custom) return custom;
    if (url.pathname === '/api/forge/account/membership') return jsonResponse(membership);
    if (url.pathname === '/api/forge/account/usage') return jsonResponse(page);
    if (url.pathname === '/api/forge/account/usage/daily') return jsonResponse({ items: [] });
    unexpected.push(method + ' ' + url.pathname);
    throw new Error('Unexpected endpoint');
  });
  vi.stubGlobal('fetch', fetch);
  return requests;
}

beforeEach(() => {
  unexpected = [];
  useAccountStore.setState({
    ...initial, status: makeAccountStatus({ loggedIn: true, user }), loading: false, error: null,
    refreshStatus: vi.fn(async () => null), openAuth: vi.fn(),
  }, true);
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  useAccountStore.setState(initial, true);
  expect(unexpected).toEqual([]);
});

describe('read-only billing usage', () => {
  it('requires login and never queries purchases or orders', async () => {
    useAccountStore.setState({ status: makeAccountStatus() });
    const requests = api();
    render(<BillingPage />);
    fireEvent.click(screen.getByTestId('billing-login'));
    expect(useAccountStore.getState().openAuth).toHaveBeenCalledOnce();
    expect(requests).toEqual([]);
  });

  it('uses the actual cycle and per-call records, with full-range totals and precise small costs', async () => {
    const requests = api();
    render(<BillingPage />);
    const row = await screen.findByTestId('usage-call-31');
    expect(row).toHaveTextContent('research-model');
    expect(row).toHaveTextContent('1,000');
    expect(row).toHaveTextContent('850');
    expect(row).toHaveTextContent('85.0%');
    expect(row).toHaveTextContent('1.24s');
    expect(row).toHaveTextContent('0.000042');
    expect(screen.getByRole('columnheader', { name: '费用 · USD' })).toBeInTheDocument();
    expect(screen.getByTestId('usage-summary')).toHaveTextContent('125,000');
    expect(screen.getByTestId('usage-summary')).toHaveTextContent('60.0%');
    expect(screen.getByTestId('usage-summary')).toHaveTextContent('0.30 USD');
    const usage = requests.find((request) => request.path.endsWith('/usage'))!;
    expect(Object.fromEntries(usage.query)).toEqual({ from: cycle.start, to: cycle.end, limit: '10', offset: '0' });
    expect(requests.every((request) => request.method === 'GET')).toBe(true);
    expect(requests.map((request) => request.path)).toEqual(['/api/forge/account/membership', '/api/forge/account/usage']);
  });

  it('paginates calls while retaining the backend totals for the whole cycle', async () => {
    const requests = api((path, query) => path.endsWith('/usage') && query.get('offset') === '10'
      ? jsonResponse({ ...page, items: [{ ...item, id: 32, model: 'next-page-model' }] }) : undefined);
    render(<BillingPage />);
    await screen.findByTestId('usage-call-31');
    fireEvent.click(screen.getByTestId('account-usage-next'));
    await screen.findByTestId('usage-call-32');
    expect(screen.queryByTestId('usage-call-31')).not.toBeInTheDocument();
    expect(screen.getByTestId('usage-summary')).toHaveTextContent('125,000');
    expect(screen.getByTestId('account-usage-page')).toHaveTextContent('第 2 / 6 页');
    expect(requests.filter((request) => request.path.endsWith('/usage')).map((request) => request.query.get('offset'))).toEqual(['0', '10']);
  });

  it('does not request unbounded records when the cycle is missing', async () => {
    const requests = api((path) => path.endsWith('/membership') ? jsonResponse({ ...membership, cycle: {} }) : undefined);
    render(<BillingPage />);
    expect(await screen.findByTestId('billing-usage-error')).toHaveTextContent('有效的用量周期');
    expect(requests.some((request) => request.path.endsWith('/usage'))).toBe(false);
  });

  it('shows load failures and retries instead of presenting them as zero usage', async () => {
    let fail = true;
    api((path) => fail && path.endsWith('/usage') ? jsonResponse({ error: { message: '汇总读取失败', code: 'READ_FAILED' } }, false, 503) : undefined);
    render(<BillingPage />);
    expect(await screen.findByRole('alert')).toHaveTextContent('汇总读取失败');
    expect(screen.getByTestId('usage-summary')).toHaveTextContent('—');
    fail = false;
    fireEvent.click(within(screen.getByRole('alert')).getByRole('button', { name: '重试' }));
    await screen.findByTestId('usage-call-31');
  });

  it('isolates membership and records when the signed-in account changes', async () => {
    let second = false;
    api((path) => second && path.endsWith('/usage')
      ? jsonResponse({ items: [], total: 0, summary: { ...summary, requests: 0 } }) : undefined);
    render(<BillingPage />);
    await screen.findByTestId('usage-call-31');
    second = true;
    act(() => useAccountStore.setState({ status: makeAccountStatus({ loggedIn: true, user: { ...user, id: 8 } }) }));
    expect(screen.queryByTestId('usage-call-31')).not.toBeInTheDocument();
    await screen.findByTestId('account-usage-empty');
  });
});

describe('usage data windows', () => {
  it('matches the 30 UTC dates used by the backend, including month boundaries', () => {
    const today = new Date('2026-10-07T07:00:00Z');
    expect(recentUsageRange(today)).toEqual({ from: '2026-09-08T00:00:00.000Z', to: '2026-10-08T00:00:00.000Z' });
    const days = fillDays([{ date: '2026-10-06', requests: 1, inputTokens: 20, outputTokens: 5, costMicros: 42 }], 30, today);
    expect(days).toHaveLength(30);
    expect(days[0].date).toBe('2026-09-08');
    expect(days[28].inputTokens).toBe(20);
    expect(days[29].date).toBe('2026-10-07');
  });

  it('does not invent a cache percentage for missing or zero input usage', () => {
    expect(cacheHitRate(1000, 850)).toBe('85.0%');
    expect(cacheHitRate(1000, 0)).toBe('0.0%');
    expect(cacheHitRate(0, 0)).toBe('—');
    expect(cacheHitRate(1000, undefined)).toBe('—');
    expect(cacheHitRate(Number.NaN, 1)).toBe('—');
  });

  it('does not extrapolate full-range statistics from one page when summary is missing', async () => {
    api((path) => path.endsWith('/usage') ? jsonResponse({ items: [item], total: 51 }) : undefined);
    render(<UsageDashboard currency="USD" from={cycle.start} to={cycle.end} />);
    await screen.findByTestId('usage-call-31');
    const totals = screen.getByTestId('usage-summary');
    expect(totals).toHaveTextContent('—');
    expect(totals).not.toHaveTextContent('1,000');
  });

  it('ignores a delayed response for an old date range', async () => {
    let resolveOld!: (response: Response) => void;
    const old = new Promise<Response>((resolve) => { resolveOld = resolve; });
    api((path, query) => path.endsWith('/usage') && query.get('from') === cycle.start ? old
      : path.endsWith('/usage') ? jsonResponse({ ...page, items: [{ ...item, id: 33, model: 'new-range' }] }) : undefined);
    const rendered = render(<UsageDashboard currency="USD" from={cycle.start} to={cycle.end} />);
    rendered.rerender(<UsageDashboard currency="USD" from="2026-09-01T00:00:00Z" to={cycle.end} />);
    await screen.findByTestId('usage-call-33');
    await act(async () => { resolveOld(jsonResponse(page)); await old; });
    expect(screen.queryByTestId('usage-call-31')).not.toBeInTheDocument();
    expect(screen.getByTestId('usage-call-33')).toHaveTextContent('new-range');
  });

  it('renders a year of real token activity and switches daily, weekly and cumulative views', async () => {
    const date = new Date().toISOString().slice(0, 10);
    const requests = api((path) => path.endsWith('/daily') ? jsonResponse({ items: [{ date, requests: 1, inputTokens: 1000, outputTokens: 64, costMicros: 42 }] }) : undefined);
    render(<UsageDashboard currency="USD" {...recentUsageRange()} chart />);
    const cell = await screen.findByTestId('account-activity-' + date);
    expect(cell).toHaveAttribute('title', expect.stringContaining('1,064 Token'));
    expect(cell).toHaveAttribute('data-level', '4');
    expect(screen.getAllByTestId(/^account-activity-/)).toHaveLength(ACTIVITY_DAYS);
    expect(requests.find((request) => request.path.endsWith('/daily'))?.query.get('days')).toBe('365');
    expect(screen.getByRole('group', { name: '近一年每天 Token 日期热力图' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '每周' }));
    expect(screen.getAllByTestId(/^account-activity-/)).toHaveLength(53);
    expect(screen.getByRole('group', { name: '近一年每周 Token 日期热力图' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '累计总量' }));
    expect(screen.getByTestId('account-activity-' + date)).toHaveAttribute('title', expect.stringContaining('近一年累计 1,064 Token'));
    fireEvent.click(screen.getByRole('button', { name: '每天' }));
    const restored = screen.getByTestId('account-activity-' + date);
    restored.focus();
    fireEvent.keyDown(restored, { key: 'ArrowLeft' });
    const previousWeek = new Date(Date.parse(date) - 7 * 86_400_000).toISOString().slice(0, 10);
    expect(screen.getByTestId('account-activity-' + previousWeek)).toHaveFocus();
    fireEvent.click(screen.getByTestId('account-activity-' + previousWeek));
    expect(screen.getByRole('status')).toHaveTextContent(previousWeek + ' · 0 Token');
  });

  it('removes account call details and device controls while retaining the full-range summary', async () => {
    const requests = api();
    render(<AccountPage />);
    await screen.findByTestId('account-usage-activity');
    await waitFor(() => expect(screen.getByTestId('usage-summary')).toHaveTextContent('125,000'));
    expect(screen.queryByText('逐次模型调用')).not.toBeInTheDocument();
    expect(screen.queryByTestId('usage-calls-table')).not.toBeInTheDocument();
    expect(screen.queryByTestId('account-usage-page')).not.toBeInTheDocument();
    expect(screen.queryByText('登录设备')).not.toBeInTheDocument();
    expect(screen.queryByTestId('account-devices-card')).not.toBeInTheDocument();
    expect(requests.some((request) => request.path.includes('/devices'))).toBe(false);
    expect(requests.find((request) => request.path.endsWith('/usage'))?.query.get('limit')).toBe('1');
    expect(screen.getAllByTestId(/^account-activity-/).every((cell) => cell.getAttribute('data-level') === '0')).toBe(true);
  });

  it('reports account summary failures even when call details are hidden', async () => {
    api((path) => path.endsWith('/usage') ? jsonResponse({ error: { message: '汇总暂不可用' } }, false, 503) : undefined);
    render(<AccountPage />);
    expect(await screen.findByRole('alert')).toHaveTextContent('用量汇总加载失败：汇总暂不可用');
    expect(screen.getByTestId('usage-summary')).toHaveTextContent('—');
  });

  it('aggregates calendar weeks and cumulative values without inventing future dates or out-of-range usage', () => {
    const day = (date: string, inputTokens: number) => ({ date, inputTokens, outputTokens: 0, requests: 1, costMicros: 0 });
    const items = [day('2025-01-01', 999), day('2026-10-03', 30), day('2026-10-04', 60), day('2026-10-06', 90), day('2026-10-08', 999)];
    const today = new Date('2026-10-07T23:00:00Z');
    const daily = buildUsageActivity(items, 'daily', today);
    expect(daily.firstDate).toBe('2025-10-08');
    expect(daily.lastDate).toBe('2026-10-07');
    expect(daily.total).toBe(180);
    expect(daily.cells.find((cell) => cell.date === '2026-10-05')?.value).toBe(0);
    expect(daily.months.map((month) => month.label)).toEqual(['11月', '12月', '1月', '2月', '3月', '4月', '5月', '6月', '7月', '8月', '9月', '10月']);
    const weekly = buildUsageActivity(items, 'weekly', today);
    expect(weekly.cells.at(-2)).toMatchObject({ date: '2026-09-27', endDate: '2026-10-03', value: 30 });
    expect(weekly.cells.at(-1)).toMatchObject({ date: '2026-10-04', endDate: '2026-10-07', value: 150 });
    const cumulative = buildUsageActivity(items, 'total', today);
    expect(cumulative.cells.find((cell) => cell.date === '2026-10-04')?.value).toBe(90);
    expect(cumulative.cells.at(-1)?.value).toBe(180);
    expect(activityLevel(0, 180)).toBe(0);
    expect(activityLevel(30, 180)).toBe(1);
    expect(activityLevel(180, 180)).toBe(4);
  });

  it('keeps leap-day cells aligned with their UTC weekdays', () => {
    const result = buildUsageActivity([], 'daily', new Date('2024-03-02T01:00:00Z'));
    expect(result.cells).toHaveLength(365);
    expect(result.cells.find((cell) => cell.date === '2024-02-29')).toMatchObject({ row: 4, value: 0 });
    expect(result.cells.find((cell) => cell.date === '2024-03-01')).toMatchObject({ row: 5, value: 0 });
  });
});

