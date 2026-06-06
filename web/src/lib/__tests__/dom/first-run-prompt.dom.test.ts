// @vitest-environment happy-dom
/**
 * DOM tests for the first-run prompt (Phase 4): it renders the welcome +
 * station search, and is dismissable by the × button and by Escape — calling
 * onDone (which the page uses to persist the "onboarded" flag). It must never
 * gate the board, so dismissal is always available.
 */
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import { mockInvoke, resetMockHandlers } from '$lib/ipc/mock.js';
import FirstRunPrompt from '../../components/FirstRunPrompt.svelte';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));

describe('FirstRunPrompt', () => {
  beforeEach(() => {
    resetMockHandlers();
  });

  it('renders the welcome and a station search', () => {
    render(FirstRunPrompt, { onDone: () => undefined });
    expect(screen.getByText(/welcome to tubbie/i)).toBeTruthy();
    // StationSearch renders a combobox/textbox input.
    expect(document.querySelector('input')).not.toBeNull();
  });

  it('the × button dismisses (calls onDone)', async () => {
    const onDone = vi.fn();
    render(FirstRunPrompt, { onDone });
    await fireEvent.click(screen.getByRole('button', { name: /dismiss welcome/i }));
    expect(onDone).toHaveBeenCalledOnce();
  });

  it('Escape dismisses (calls onDone) — no keyboard trap', async () => {
    const onDone = vi.fn();
    render(FirstRunPrompt, { onDone });
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(onDone).toHaveBeenCalledOnce();
  });
});
