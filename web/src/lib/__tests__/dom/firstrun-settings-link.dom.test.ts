// @vitest-environment happy-dom
/**
 * The first-run prompt's "Settings" link must dismiss the prompt (onDone) AND
 * open the in-frame Settings panel — in that order, so the prompt doesn't
 * linger behind the overlay (FirstRunPrompt.svelte goToSettings()). This is
 * deliberate sequencing that would rot silently without a test.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import { get } from 'svelte/store';
import { mockInvoke } from '$lib/ipc/mock.js';
import { settingsOpen } from '$lib/stores/settingsView.js';
import FirstRunPrompt from '$lib/components/FirstRunPrompt.svelte';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));

describe('FirstRunPrompt — Settings link', () => {
  beforeEach(() => {
    settingsOpen.set(false);
  });

  it('dismisses the prompt (onDone) and opens the Settings panel', async () => {
    const onDone = vi.fn();
    render(FirstRunPrompt, { props: { onDone } });
    const link = screen.getByRole('button', { name: /^settings$/i });
    await fireEvent.click(link);
    expect(onDone).toHaveBeenCalledOnce();
    expect(get(settingsOpen)).toBe(true);
  });
});
