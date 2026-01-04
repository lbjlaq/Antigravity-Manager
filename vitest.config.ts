import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  test: {
    // Use jsdom for React component testing
    environment: 'jsdom',

    // Global test APIs (describe, it, expect) - no need to import
    globals: true,

    // Setup files for extending matchers
    setupFiles: ['./src/test/setup.ts'],

    // Include patterns
    include: ['src/**/*.{test,spec}.{ts,tsx}'],

    // Coverage configuration
    coverage: {
      provider: 'v8',
      reporter: ['text', 'json', 'html'],
      include: ['src/**/*.{ts,tsx}'],
      exclude: [
        'src/**/*.test.{ts,tsx}',
        'src/**/*.spec.{ts,tsx}',
        'src/test/**',
        'src/main.tsx',
        'src/vite-env.d.ts',
      ],
    },

    // Reporter configuration
    reporters: ['default'],

    // Watch mode settings
    watch: false,

    // Type checking is handled by tsc
    typecheck: {
      enabled: false,
    },
  },
});
