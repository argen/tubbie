// @vitest-environment happy-dom
/**
 * DOM tests for the Settings page — specifically:
 *   Fix 1: app key must NOT linger in renderer heap after onMount.
 *   Fix 3: configError store surfaces as a banner.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { get } from 'svelte/store';
import { mockInvoke, setMockHandler, resetMockHandlers } from '$lib/ipc/mock.js';
import { configError } from '$lib/stores/config.js';
import SettingsPage from '../../../routes/settings/+page.svelte';

// Wire Tauri API mocks.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));

// SvelteKit navigation mock is already wired via vitest alias.

describe('Settings — Fix 1: app key not in renderer heap', () => {
  beforeEach(() => {
    resetMockHandlers();
    configError.set(null);
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('input value is empty string after onMount even when a key is stored', async () => {
    setMockHandler('has_app_key', () => true);

    render(SettingsPage);

    // The API key input has id="api-key-input"
    const input = await waitFor(
      () => document.getElementById('api-key-input') as HTMLInputElement | null,
    );
    expect(input).not.toBeNull();
    // The actual value must be empty — key never loaded into DOM.
    expect(input?.value).toBe('');
  });

  it('placeholder reflects presence of a stored key', async () => {
    setMockHandler('has_app_key', () => true);

    render(SettingsPage);

    const input = await waitFor(
      () => document.getElementById('api-key-input') as HTMLInputElement | null,
    );
    expect(input).not.toBeNull();
    expect(input?.placeholder).toContain('stored');
  });

  it('placeholder reflects absence of a stored key', async () => {
    setMockHandler('has_app_key', () => false);

    render(SettingsPage);

    const input = await waitFor(
      () => document.getElementById('api-key-input') as HTMLInputElement | null,
    );
    expect(input).not.toBeNull();
    expect(input?.placeholder).not.toContain('stored');
    expect(input?.placeholder).toContain('optional');
  });

  it('load_app_key is NOT invoked during onMount', async () => {
    const loadAppKeyMock = vi.fn(() => null);
    setMockHandler('load_app_key', loadAppKeyMock);
    setMockHandler('has_app_key', () => false);

    render(SettingsPage);

    // Wait for onMount (has_app_key) to complete.
    await waitFor(() => document.getElementById('api-key-input'));

    expect(loadAppKeyMock).not.toHaveBeenCalled();
  });

  it('appKey is cleared from state immediately after save', async () => {
    setMockHandler('has_app_key', () => false);
    setMockHandler('save_app_key', () => 'restart to apply');

    render(SettingsPage);

    const input = await waitFor(
      () => document.getElementById('api-key-input') as HTMLInputElement | null,
    );
    expect(input).not.toBeNull();

    // Simulate user typing a key.
    if (input) {
      await fireEvent.input(input, { target: { value: 'my-secret-key' } });
    }

    // Click "Save Key" button (aria-label: "Save API key (requires restart)").
    const saveBtn = screen.getByRole('button', { name: /save api key/i });
    await fireEvent.click(saveBtn);

    // After save, input value must be cleared.
    await waitFor(() => {
      expect(input?.value ?? '').toBe('');
    });
  });
});

describe('Settings — Fix 3: configError banner', () => {
  beforeEach(() => {
    resetMockHandlers();
    configError.set(null);
  });

  afterEach(() => {
    vi.clearAllMocks();
    configError.set(null);
  });

  it('shows configError banner when store has an error', async () => {
    configError.set('validation: station_id invalid');

    render(SettingsPage);

    const banner = await waitFor(() => screen.getByRole('alert'));
    expect(banner.textContent).toContain('validation: station_id invalid');
  });

  it('banner is absent when configError is null', async () => {
    configError.set(null);

    render(SettingsPage);

    // Give onMount time to run.
    await waitFor(() => document.getElementById('api-key-input'));

    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('dismiss button clears the configError store', async () => {
    configError.set('some error');

    render(SettingsPage);

    const dismissBtn = await waitFor(() => screen.getByRole('button', { name: /dismiss/i }));
    await fireEvent.click(dismissBtn);

    await waitFor(() => {
      expect(get(configError)).toBeNull();
    });
  });

  it('configError banner disappears after dismiss', async () => {
    configError.set('some error');

    render(SettingsPage);

    const dismissBtn = await waitFor(() => screen.getByRole('button', { name: /dismiss/i }));
    await fireEvent.click(dismissBtn);

    await waitFor(() => {
      expect(screen.queryByRole('alert')).toBeNull();
    });
  });

  it('save_config failure causes configError to show in the store', async () => {
    // updateConfig sets configError when save_config throws.
    setMockHandler('save_config', () => {
      throw new Error('validation: station_id invalid');
    });

    render(SettingsPage);

    // Click "Save Settings".
    const saveBtn = await waitFor(() => screen.getByRole('button', { name: /save settings/i }));
    await fireEvent.click(saveBtn);

    // configError store should be populated.
    await waitFor(() => {
      const err = get(configError);
      expect(err).not.toBeNull();
      expect(err).toContain('station_id invalid');
    });
  });
});
