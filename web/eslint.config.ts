import { defineConfig, globalIgnores } from 'eslint/config';
import globals from 'globals';
import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import svelte from 'eslint-plugin-svelte';
import prettier from 'eslint-config-prettier';
import svelteConfig from './svelte.config.js';

export default defineConfig(
  // Global ignores
  globalIgnores(['.svelte-kit/', 'dist/', 'build/', 'coverage/', 'node_modules/', 'src-tauri/']),

  // JS recommended base
  js.configs.recommended,

  // TypeScript type-checked strict + stylistic
  tseslint.configs.strictTypeChecked,
  tseslint.configs.stylisticTypeChecked,

  // Svelte plugin
  svelte.configs.recommended,

  // Browser + Node globals for all files
  {
    languageOptions: {
      globals: {
        ...globals.browser,
        ...globals.node,
      },
      parserOptions: {
        projectService: true,
        extraFileExtensions: ['.svelte'],
      },
    },
  },

  // Svelte-specific: configure TypeScript parser inside .svelte files
  {
    files: ['**/*.svelte', '**/*.svelte.ts', '**/*.svelte.js'],
    languageOptions: {
      parserOptions: {
        projectService: true,
        extraFileExtensions: ['.svelte'],
        parser: tseslint.parser,
        svelteConfig,
      },
    },
    rules: {
      // Tauri desktop app: routing is client-side only with no base-path concern.
      // resolve() from $app/paths is not needed here.
      'svelte/no-navigation-without-resolve': 'off',
    },
  },

  // Root config files — disable type-checked rules (no project reference needed)
  {
    files: ['*.config.ts', '*.config.js', '*.config.mjs'],
    extends: [tseslint.configs.disableTypeChecked],
  },

  // Global rule overrides
  {
    rules: {
      // Allow underscore-prefixed variables/params as intentionally unused
      '@typescript-eslint/no-unused-vars': [
        'error',
        {
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          caughtErrorsIgnorePattern: '^_',
        },
      ],
    },
  },

  // Test file overrides — relax a few rules that are noisy in test contexts
  {
    files: ['**/__tests__/**/*.ts', '**/*.test.ts', '**/*.spec.ts', '**/__mocks__/**/*.ts'],
    rules: {
      '@typescript-eslint/no-unsafe-assignment': 'off',
      '@typescript-eslint/no-unsafe-member-access': 'off',
      '@typescript-eslint/no-non-null-assertion': 'off',
      '@typescript-eslint/unbound-method': 'off',
      '@typescript-eslint/no-unsafe-type-assertion': 'off',
      '@typescript-eslint/no-unnecessary-condition': 'off',
      '@typescript-eslint/no-unnecessary-type-assertion': 'off',
      '@typescript-eslint/require-await': 'off',
    },
  },

  // Prettier last — disables rules that conflict with Prettier formatting
  prettier,
);
