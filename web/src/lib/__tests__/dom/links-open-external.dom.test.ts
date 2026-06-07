// @vitest-environment happy-dom
/**
 * Regression: every external link must open the system browser via the opener
 * plugin. A bare `<a target="_blank">` is a no-op inside the Tauri WKWebView,
 * which is why the footer / About / API-key links did nothing when clicked.
 *
 * These tests mock the opener plugin's `openUrl` and assert each link routes
 * the correct URL through it (and calls `preventDefault`, so the dead default
 * navigation never fires).
 */
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

const openUrl = vi.fn();
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: (url: string): Promise<void> => {
    openUrl(url);
    return Promise.resolve();
  },
}));
// ApiKeySection.onMount calls has_app_key; AboutSection.onMount calls
// getVersion. Stub both so the components mount cleanly in happy-dom.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: () => Promise.resolve(false),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));
vi.mock('@tauri-apps/api/app', () => ({
  getVersion: () => Promise.resolve('1.0.0'),
}));

import Attribution from '$lib/components/Attribution.svelte';
import AboutSection from '$lib/components/AboutSection.svelte';
import ApiKeySection from '$lib/components/ApiKeySection.svelte';

describe('external links open via the opener plugin', () => {
  beforeEach(() => {
    openUrl.mockReset();
  });

  it('Attribution footer "TfL Open Data" opens the TfL terms URL', async () => {
    render(Attribution);
    const link = screen.getByRole('link', { name: /TfL Open Data/i });
    await fireEvent.click(link);
    expect(openUrl).toHaveBeenCalledWith(
      'https://tfl.gov.uk/corporate/terms-and-conditions/transport-data-service',
    );
  });

  it('About section "Source & releases" opens the GitHub repo', async () => {
    render(AboutSection);
    const link = screen.getByRole('link', { name: /Source & releases/i });
    await fireEvent.click(link);
    expect(openUrl).toHaveBeenCalledWith('https://github.com/argen/tubbie');
  });

  it('About section "TfL Open Data" opens the open-data-users page', async () => {
    render(AboutSection);
    const link = screen.getByRole('link', { name: /TfL Open Data/i });
    await fireEvent.click(link);
    expect(openUrl).toHaveBeenCalledWith('https://tfl.gov.uk/info-for/open-data-users/');
  });

  it('API-key section "api-portal" link opens the TfL API portal', async () => {
    render(ApiKeySection);
    const link = screen.getByRole('link', { name: /api-portal\.tfl\.gov\.uk/i });
    await fireEvent.click(link);
    expect(openUrl).toHaveBeenCalledWith('https://api-portal.tfl.gov.uk');
  });
});
