import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { resolve } from 'path';

const alias = {
  $lib: resolve(__dirname, 'src/lib'),
  '$app/navigation': resolve(__dirname, 'src/lib/__mocks__/app-navigation.ts'),
};

export default defineConfig({
  plugins: [svelte()],
  resolve: { alias },
  test: {
    // Default environment: node — fast, for pure logic tests.
    environment: 'node',
    // Vitest 4 uses `projects` to run different environments
    projects: [
      {
        // Pure logic tests: node env
        plugins: [svelte()],
        resolve: { alias },
        test: {
          name: 'unit',
          environment: 'node',
          // Co-located TS-core tests (src/lib/tfl/**) run here too: pure logic +
          // Node-fs fixture loading, no DOM. This glob is what makes every later
          // port phase's tests collectable.
          include: [
            'src/lib/__tests__/*.test.ts',
            'src/lib/__tests__/*.spec.ts',
            'src/lib/tfl/**/*.test.ts',
          ],
        },
      },
      {
        // DOM component tests: happy-dom
        // Must use browser-side Svelte resolution
        plugins: [svelte()],
        resolve: {
          alias,
          conditions: ['browser', 'module', 'import', 'default'],
        },
        test: {
          name: 'dom',
          environment: 'happy-dom',
          include: ['src/lib/__tests__/dom/*.dom.test.ts'],
          setupFiles: ['src/lib/__tests__/dom/setup.ts'],
        },
      },
    ],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'lcov'],
      include: ['src/**/*.{ts,svelte}'],
      exclude: ['src/**/*.test.ts', 'src/**/__tests__/**', 'src/app.d.ts'],
    },
  },
});
