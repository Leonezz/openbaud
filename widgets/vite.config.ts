import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// Dev-server config for the harness (`pnpm harness`). Production single-file
// bundles are built by scripts/build.mjs with its own inline config.
export default defineConfig({
  plugins: [react()],
  server: {
    open: '/harness/index.html',
  },
})
