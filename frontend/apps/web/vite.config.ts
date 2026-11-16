import react from '@vitejs/plugin-react'
import { configDefaults, defineConfig } from 'vitest/config'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  build: {
    rolldownOptions: {
      output: {
        // Keep the rarely changing framework in its own cacheable chunk, apart from application code.
        codeSplitting: { groups: [{ name: 'react', test: /node_modules[/\\](react|react-dom|scheduler)[/\\]/ }] },
      },
    },
  },
  server: {
    proxy: {
      '/api': { target: 'http://127.0.0.1:8080', changeOrigin: true, ws: true },
    },
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test-setup.ts'],
    globals: false,
    exclude: [...configDefaults.exclude, 'e2e/**'],
  },
})
