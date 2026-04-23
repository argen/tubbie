import { defineConfig, globalIgnores } from 'eslint/config';
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

  // Type-aware config for TS files
  {
    languageOptions: {
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
  },

  // Root config files — disable type-checked rules (no project reference needed)
  {
    files: ['*.config.ts', '*.config.js', '*.config.mjs'],
    extends: [tseslint.configs.disableTypeChecked],
  },

  // Test file overrides — relax a few rules that are noisy in test contexts
  {
    files: ['**/__tests__/**/*.ts', '**/*.test.ts', '**/*.spec.ts'],
    rules: {
      '@typescript-eslint/no-unsafe-assignment': 'off',
      '@typescript-eslint/no-unsafe-member-access': 'off',
      '@typescript-eslint/no-non-null-assertion': 'off',
      '@typescript-eslint/unbound-method': 'off',
      '@typescript-eslint/no-unsafe-type-assertion': 'off',
    },
  },

  // Prettier last — disables rules that conflict with Prettier formatting
  prettier,
);
