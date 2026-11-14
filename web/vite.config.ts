import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [
    sveltekit({
      preprocess: vitePreprocess(),

      // Tauri loads from tauri://localhost — SPA mode with index.html fallback.
      // All client-side routing is handled by SvelteKit's router in the webview.
      adapter: adapter({
        fallback: 'index.html',
      }),
      prerender: {
        // /settings is explicitly prerendered so the settings webview window
        // (WebviewUrl::App("/settings")) loads web/build/settings/index.html
        // directly rather than falling through to the SPA fallback and
        // flash-routing from the board page to settings.
        // The route also sets `export const prerender = true` in +page.ts.
        entries: ['*', '/settings'],
        // Suppress 404 for static assets that are not page routes (e.g.
        // /favicon.png is referenced from app.html but is not served in
        // development — Tauri bundles the icon separately via tauri.conf.json).
        handleHttpError: ({ path, referrer: _referrer, message }) => {
          if (path === '/favicon.png') return;
          throw new Error(message);
        },
      },
    }),
  ],
});
