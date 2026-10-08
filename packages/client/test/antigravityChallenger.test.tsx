import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  AntigravityCard,
  AntigravityRateLimitBar,
  formatResetTime,
  parseAntigravityLimits,
  type AntigravityLimitBucket,
} from '@/components/settings/ModelsPage';
import { type AntigravityQuota, type AntigravityStatus } from '@/lib/forgeApi';
import { mockForgeBackend } from './forgeMock';

describe('Challenger: RateLimitBar Mathematical Edge Cases', () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  describe('parseAntigravityLimits Edge Cases', () => {
    it('0% remaining: parses exact 0 remaining and handles 100% used or >100% burst used', () => {
      // Direct 0% remaining
      const b1 = parseAntigravityLimits({
        primary: { remainingPercent: 0 },
      });
      expect(b1).toHaveLength(1);
      expect(b1[0].remainingPercent).toBe(0);

      // 100% used -> 0% remaining
      const b2 = parseAntigravityLimits({
        primary: { usedPercent: 100 },
      });
      expect(b2).toHaveLength(1);
      expect(b2[0].remainingPercent).toBe(0);

      // 150% used (burst quota exceeded) -> clamps to 0% remaining (no negative)
      const b3 = parseAntigravityLimits({
        primary: { usedPercent: 150 },
      });
      expect(b3).toHaveLength(1);
      expect(b3[0].remainingPercent).toBe(0);
    });

    it('100% remaining: parses exact 100 remaining, 0% used, and negative usedPercent', () => {
      // Direct 100% remaining
      const b1 = parseAntigravityLimits({
        primary: { remainingPercent: 100 },
      });
      expect(b1).toHaveLength(1);
      expect(b1[0].remainingPercent).toBe(100);

      // 0% used -> 100% remaining
      const b2 = parseAntigravityLimits({
        primary: { usedPercent: 0 },
      });
      expect(b2).toHaveLength(1);
      expect(b2[0].remainingPercent).toBe(100);

      // -20% used (anomalous negative usage) -> clamped to 100% max
      const b3 = parseAntigravityLimits({
        primary: { usedPercent: -20 },
      });
      expect(b3).toHaveLength(1);
      expect(b3[0].remainingPercent).toBe(100);

      // >100% remaining (e.g. 125% bonus) -> clamped to 100% max
      const b4 = parseAntigravityLimits({
        primary: { remainingPercent: 125 },
      });
      expect(b4).toHaveLength(1);
      expect(b4[0].remainingPercent).toBe(100);
    });

    it('Negative percentages: clamps negative remainingPercent to 0', () => {
      const b = parseAntigravityLimits({
        primary: { remainingPercent: -15 },
        secondary: { remainingPercent: -0.5 },
      });
      expect(b).toHaveLength(2);
      expect(b[0].remainingPercent).toBe(0);
      expect(b[1].remainingPercent).toBe(0);
    });

    it('Missing primary or secondary bucket: handles partial or completely absent buckets', () => {
      // Only primary present
      const onlyPrimary = parseAntigravityLimits({
        primary: { remainingPercent: 42, resetsAt: 1791316800 },
      });
      expect(onlyPrimary).toHaveLength(1);
      expect(onlyPrimary[0].id).toBe('primary');
      expect(onlyPrimary[0].remainingPercent).toBe(42);

      // Only secondary present
      const onlySecondary = parseAntigravityLimits({
        secondary: { remainingPercent: 77 },
      });
      expect(onlySecondary).toHaveLength(1);
      expect(onlySecondary[0].id).toBe('secondary');
      expect(onlySecondary[0].remainingPercent).toBe(77);

      // Both missing (empty object)
      expect(parseAntigravityLimits({})).toEqual([]);

      // Null or undefined
      expect(parseAntigravityLimits(null)).toEqual([]);
      expect(parseAntigravityLimits(undefined)).toEqual([]);

      // Primary exists but has neither remainingPercent nor usedPercent -> defaults to 100%
      const emptyBucket = parseAntigravityLimits({
        primary: {},
      });
      expect(emptyBucket).toHaveLength(1);
      expect(emptyBucket[0].remainingPercent).toBe(100);
    });
  });

  describe('formatResetTime Timestamp Edge Cases', () => {
    it('Unix epoch seconds vs milliseconds vs ISO strings vs invalid inputs', () => {
      // 1. Epoch seconds (typical 10 digits < 1e11)
      const secFormatted = formatResetTime(1791316800);
      expect(secFormatted).toMatch(/^重置/);

      // 2. Epoch milliseconds (13 digits >= 1e11)
      const msFormatted = formatResetTime(1791316800000);
      expect(msFormatted).toMatch(/^重置/);
      expect(msFormatted).toBe(secFormatted);

      // 3. ISO string formats (UTC and timezone offset)
      const isoUtc = formatResetTime('2026-10-06T20:00:00Z');
      expect(isoUtc).toMatch(/^重置/);
      const isoWithOffset = formatResetTime('2026-10-06T20:00:00+08:00');
      expect(isoWithOffset).toMatch(/^重置/);

      // 4. Invalid inputs
      expect(formatResetTime(undefined)).toBe('');
      expect(formatResetTime(null as unknown as string)).toBe('');
      expect(formatResetTime('')).toBe('');
      expect(formatResetTime('not-a-timestamp')).toBe('');
      expect(formatResetTime(NaN)).toBe('');
    });
  });

  describe('AntigravityRateLimitBar Visual and DOM Rendering', () => {
    it('renders 0% remaining with critical alert color bg-red-500 and 0% bar width', () => {
      const bucket: AntigravityLimitBucket = {
        id: 'primary',
        label: '主要额度',
        remainingPercent: 0,
      };
      render(<AntigravityRateLimitBar bucket={bucket} />);

      const el = screen.getByTestId('antigravity-limit-primary');
      expect(el).toHaveTextContent('主要额度');
      expect(el).toHaveTextContent('剩余 0%');

      const bar = el.querySelector('.bg-red-500');
      expect(bar).not.toBeNull();
      expect(bar).toHaveStyle({ width: '0%' });
    });

    it('renders 100% remaining with sage color bg-sage and 100% bar width', () => {
      const bucket: AntigravityLimitBucket = {
        id: 'secondary',
        label: '次要额度',
        remainingPercent: 100,
      };
      render(<AntigravityRateLimitBar bucket={bucket} />);

      const el = screen.getByTestId('antigravity-limit-secondary');
      expect(el).toHaveTextContent('剩余 100%');

      const bar = el.querySelector('.bg-sage');
      expect(bar).not.toBeNull();
      expect(bar).toHaveStyle({ width: '100%' });
    });

    it('renders 15% remaining with warn color bg-warn (5 < x <= 20)', () => {
      const bucket: AntigravityLimitBucket = {
        id: 'primary',
        label: '主要额度',
        remainingPercent: 15,
      };
      render(<AntigravityRateLimitBar bucket={bucket} />);

      const el = screen.getByTestId('antigravity-limit-primary');
      expect(el).toHaveTextContent('剩余 15%');

      const bar = el.querySelector('.bg-warn');
      expect(bar).not.toBeNull();
      expect(bar).toHaveStyle({ width: '15%' });
    });

    it('renders reset countdown when resetsAt is valid ISO string', () => {
      const bucket: AntigravityLimitBucket = {
        id: 'primary',
        label: '主要额度',
        remainingPercent: 50,
        resetsAt: '2026-10-06T20:00:00Z',
      };
      render(<AntigravityRateLimitBar bucket={bucket} />);

      const el = screen.getByTestId('antigravity-limit-primary');
      expect(el.textContent).toMatch(/重置/);
    });

    it('omits reset countdown when resetsAt is missing or invalid', () => {
      const bucket: AntigravityLimitBucket = {
        id: 'primary',
        label: '主要额度',
        remainingPercent: 50,
        resetsAt: 'invalid-date',
      };
      render(<AntigravityRateLimitBar bucket={bucket} />);

      const el = screen.getByTestId('antigravity-limit-primary');
      expect(el.textContent).not.toMatch(/重置/);
    });
  });
});

describe('Challenger: Configuration Drawer Interaction & Validation', () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  const setupCard = (initialStatusOverrides?: Partial<AntigravityStatus>) => {
    let savedConfig: Record<string, unknown> | null = null;
    let probedConfig: Record<string, unknown> | null = null;

    const baseStatus: AntigravityStatus = {
      configured: false,
      baseUrl: '',
      model: 'gemini-3.8-flash',
      keyConfigured: false,
      availability: 'needs-config',
      ...initialStatusOverrides,
    };

    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/llm/antigravity/status': baseStatus,
          '/api/forge/llm/antigravity/config': (init?: { body?: string }) => {
            savedConfig = JSON.parse(init?.body ?? '{}');
            return {
              ok: true,
              configured: true,
              baseUrl: savedConfig?.baseUrl,
              model: savedConfig?.model,
              keyConfigured: Boolean(savedConfig?.key || baseStatus.keyConfigured),
            };
          },
          '/api/forge/llm/antigravity/probe': (init?: { body?: string }) => {
            probedConfig = JSON.parse(init?.body ?? '{}');
            return {
              ok: true,
              status: 'available',
              latencyMs: 45,
            };
          },
        },
      ),
    );

    return {
      getSavedConfig: () => savedConfig,
      getProbedConfig: () => probedConfig,
    };
  };

  it('Quick presets: clicking gemini-3.8-pro and gemini-3.8-flash updates model input and active styles', async () => {
    setupCard();
    render(<AntigravityCard />);

    // Open drawer
    fireEvent.click(screen.getByTestId('antigravity-config-toggle'));

    const modelInput = screen.getByTestId('antigravity-model-input') as HTMLInputElement;
    const presetPro = screen.getByTestId('antigravity-preset-gemini-3.8-pro');
    const presetFlash = screen.getByTestId('antigravity-preset-gemini-3.8-flash');

    // Default model is gemini-3.8-flash
    expect(modelInput.value).toBe('gemini-3.8-flash');
    expect(presetFlash.className).toContain('border-acc');
    expect(presetPro.className).not.toContain('border-acc');

    // Switch to gemini-3.8-pro
    fireEvent.click(presetPro);
    expect(modelInput.value).toBe('gemini-3.8-pro');
    expect(presetPro.className).toContain('border-acc');
    expect(presetFlash.className).not.toContain('border-acc');

    // Switch back to gemini-3.8-flash
    fireEvent.click(presetFlash);
    expect(modelInput.value).toBe('gemini-3.8-flash');
    expect(presetFlash.className).toContain('border-acc');
    expect(presetPro.className).not.toContain('border-acc');
  });

  it('Validation: empty or whitespace-only baseUrl disables Save button and prevents submit', async () => {
    const { getSavedConfig } = setupCard();
    render(<AntigravityCard />);

    // Open drawer
    fireEvent.click(screen.getByTestId('antigravity-config-toggle'));

    const baseUrlInput = screen.getByTestId('antigravity-baseurl-input');
    const modelInput = screen.getByTestId('antigravity-model-input');
    const saveBtn = screen.getByTestId('antigravity-config-save');

    // Initially baseUrl is empty -> Save button is disabled
    expect(saveBtn).toBeDisabled();
    fireEvent.click(saveBtn);
    expect(getSavedConfig()).toBeNull();

    // Set whitespace-only baseUrl -> still disabled
    fireEvent.change(baseUrlInput, { target: { value: '   ' } });
    expect(saveBtn).toBeDisabled();
    fireEvent.click(saveBtn);
    expect(getSavedConfig()).toBeNull();

    // Set valid baseUrl -> becomes enabled
    fireEvent.change(baseUrlInput, { target: { value: 'https://proxy.example.com' } });
    expect(saveBtn).not.toBeDisabled();

    // Now clear model input -> disabled again
    fireEvent.change(modelInput, { target: { value: '' } });
    expect(saveBtn).toBeDisabled();
    fireEvent.click(saveBtn);
    expect(getSavedConfig()).toBeNull();

    // Set whitespace-only model -> disabled
    fireEvent.change(modelInput, { target: { value: ' \t  ' } });
    expect(saveBtn).toBeDisabled();
    fireEvent.click(saveBtn);
    expect(getSavedConfig()).toBeNull();

    // Set valid model -> enabled and can save
    fireEvent.change(modelInput, { target: { value: 'gemini-3.8-pro' } });
    expect(saveBtn).not.toBeDisabled();
    fireEvent.click(saveBtn);

    await waitFor(() => {
      expect(getSavedConfig()).toEqual({
        baseUrl: 'https://proxy.example.com',
        model: 'gemini-3.8-pro',
      });
    });
  });

  it('Masked API key input: preserves existing keystore key when blank, updates when entered', async () => {
    const { getSavedConfig } = setupCard({
      configured: true,
      baseUrl: 'https://existing.proxy',
      model: 'gemini-3.8-flash',
      keyConfigured: true,
      availability: 'available',
    });

    render(<AntigravityCard />);
    await waitFor(() => {
      expect(screen.getByTestId('antigravity-availability')).toHaveTextContent('available');
    });

    fireEvent.click(screen.getByTestId('antigravity-config-toggle'));

    const keyInput = screen.getByTestId('antigravity-key-input') as HTMLInputElement;
    expect(keyInput.type).toBe('password');
    expect(keyInput.placeholder).toContain('已配置');

    const saveBtn = screen.getByTestId('antigravity-config-save');
    expect(saveBtn).not.toBeDisabled();

    // Save without modifying key -> payload should NOT contain key property
    fireEvent.click(saveBtn);
    await waitFor(() => {
      expect(getSavedConfig()).toEqual({
        baseUrl: 'https://existing.proxy',
        model: 'gemini-3.8-flash',
      });
    });
  });

  it('Whitespace trimming: baseUrl, model, and key are trimmed cleanly on save', async () => {
    const { getSavedConfig } = setupCard();
    render(<AntigravityCard />);

    fireEvent.click(screen.getByTestId('antigravity-config-toggle'));

    fireEvent.change(screen.getByTestId('antigravity-baseurl-input'), {
      target: { value: '   https://proxy.example.com/v1   ' },
    });
    fireEvent.change(screen.getByTestId('antigravity-model-input'), {
      target: { value: '   gemini-3.8-flash   ' },
    });
    fireEvent.change(screen.getByTestId('antigravity-key-input'), {
      target: { value: '   my-secret-key   ' },
    });

    fireEvent.click(screen.getByTestId('antigravity-config-save'));

    await waitFor(() => {
      expect(getSavedConfig()).toEqual({
        baseUrl: 'https://proxy.example.com/v1',
        model: 'gemini-3.8-flash',
        key: 'my-secret-key',
      });
    });
  });

  it('Key with only whitespace is omitted completely from payload', async () => {
    const { getSavedConfig } = setupCard();
    render(<AntigravityCard />);

    fireEvent.click(screen.getByTestId('antigravity-config-toggle'));

    fireEvent.change(screen.getByTestId('antigravity-baseurl-input'), {
      target: { value: 'https://proxy.example.com/v1' },
    });
    fireEvent.change(screen.getByTestId('antigravity-key-input'), {
      target: { value: '   \t  ' },
    });

    fireEvent.click(screen.getByTestId('antigravity-config-save'));

    await waitFor(() => {
      expect(getSavedConfig()).toEqual({
        baseUrl: 'https://proxy.example.com/v1',
        model: 'gemini-3.8-flash',
      });
    });
  });

  it('Custom model name removes active styling from both presets', async () => {
    setupCard();
    render(<AntigravityCard />);

    fireEvent.click(screen.getByTestId('antigravity-config-toggle'));

    const modelInput = screen.getByTestId('antigravity-model-input');
    const presetPro = screen.getByTestId('antigravity-preset-gemini-3.8-pro');
    const presetFlash = screen.getByTestId('antigravity-preset-gemini-3.8-flash');

    fireEvent.change(modelInput, { target: { value: 'gemini-1.5-pro-preview' } });

    expect(presetPro.className).not.toContain('border-acc');
    expect(presetFlash.className).not.toContain('border-acc');
  });

  it('Probe network exception: catches fetch network error without unhandled rejection or UI crash', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockImplementation(async (url: string) => {
        if (url.includes('/status')) {
          return {
            ok: true,
            status: 200,
            json: async () => ({
              configured: false,
              baseUrl: 'http://dead.host',
              model: 'gemini-3.8-flash',
              keyConfigured: false,
              availability: 'offline',
            }),
          };
        }
        if (url.includes('/probe')) {
          throw new TypeError('Failed to fetch (Network unreachable)');
        }
        return { ok: true, status: 200, json: async () => ({}) };
      }),
    );

    render(<AntigravityCard />);
    await waitFor(() => {
      expect(screen.getByTestId('antigravity-availability')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('antigravity-config-toggle'));
    fireEvent.click(screen.getByTestId('antigravity-probe-btn'));

    await waitFor(() => {
      const feedback = screen.getByTestId('antigravity-probe-feedback');
      expect(feedback).toHaveTextContent('✕ Failed to fetch (Network unreachable)');
    });
  });

  it('Quota priority: status.rateLimits takes precedence over status.quota', async () => {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/llm/antigravity/status': {
            configured: true,
            baseUrl: 'http://proxy.test',
            model: 'gemini-3.8-flash',
            keyConfigured: true,
            availability: 'available',
            rateLimits: {
              primary: { remainingPercent: 99 },
            },
            quota: {
              primary: { remainingPercent: 12 },
            },
          },
        },
      ),
    );

    render(<AntigravityCard />);

    const primaryBar = await screen.findByTestId('antigravity-limit-primary');
    // rateLimits has 99%, quota has 12% -> 99% wins
    expect(primaryBar).toHaveTextContent('剩余 99%');
  });
});

