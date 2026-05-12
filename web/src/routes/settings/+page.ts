/**
 * Prerender the /settings route so the settings webview window loads it
 * directly from `web/build/settings/index.html` rather than falling back
 * to `web/build/index.html` (the SPA root) and SPA-routing to /settings.
 *
 * Without prerendering:
 *   `WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("/settings"))`
 *   resolves `tauri://localhost/settings` via the SPA fallback index.html,
 *   the main board route paints first, then the client router navigates to
 *   /settings — causing a flash of wrong content and running the board
 *   bootstrap in the wrong context.
 *
 * With prerendering (this file):
 *   `web/build/settings/index.html` exists and Tauri serves it directly,
 *   so the settings window opens directly on the settings page.
 */
export const prerender = true;

// 'always' causes the prerendered output to be written as
// `settings/index.html` (a directory index) rather than the flat
// `settings.html`. Tauri's asset server resolves `tauri://localhost/settings`
// against the frontendDist directory; a directory index is served reliably
// by the file-based asset resolver without needing extension inference.
export const trailingSlash = 'always';
