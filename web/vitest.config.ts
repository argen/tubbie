import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  test: {
    // Default environment is node — lean and fast for unit tests.
    // For DOM tests, add `// @vitest-environment happy-dom` at the top of the
    // test file (files matching **/*.dom.test.ts by convention).
    environment: 'node',
    include: ['src/**/*.{test,spec}.ts', 'src/**/__tests__/**/*.ts'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'lcov'],
      include: ['src/**/*.{ts,svelte}'],
      exclude: ['src/**/*.test.ts', 'src/**/__tests__/**', 'src/app.d.ts'],
    },
  },
});
