import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),

  kit: {
    // Tauri loads from tauri://localhost — SPA mode with index.html fallback.
    // All client-side routing is handled by SvelteKit's router in the webview.
    adapter: adapter({
      fallback: 'index.html',
    }),
  },
};

export default config;
