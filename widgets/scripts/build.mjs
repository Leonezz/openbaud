#!/usr/bin/env node
// Builds each app under src/apps/ into a self-contained dist/<app>.html
// (vite-plugin-singlefile). Fails loudly on a missing entry or any external
// resource reference in the output.

import { existsSync, readFileSync, renameSync, rmSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import react from '@vitejs/plugin-react'
import { build } from 'vite'
import { viteSingleFile } from 'vite-plugin-singlefile'

const APPS = ['_smoke', 'port-picker', 'viewer']

const widgetsRoot = join(dirname(fileURLToPath(import.meta.url)), '..')
const distDir = join(widgetsRoot, 'dist')

rmSync(distDir, { recursive: true, force: true })

for (const app of APPS) {
  const appRoot = join(widgetsRoot, 'src', 'apps', app)
  const entry = join(appRoot, 'index.html')
  if (!existsSync(entry)) {
    throw new Error(`app ${app}: missing entry ${entry}`)
  }

  await build({
    configFile: false,
    root: appRoot,
    base: './',
    plugins: [react(), viteSingleFile()],
    logLevel: 'warn',
    build: {
      outDir: distDir,
      emptyOutDir: false,
    },
  })

  const built = join(distDir, 'index.html')
  const target = join(distDir, `${app}.html`)
  renameSync(built, target)

  const html = readFileSync(target, 'utf-8')
  // MCP Apps run under a strict CSP: the page must not load anything remote.
  const external = html.match(/(?:src|href)\s*=\s*["']https?:|url\(\s*["']?https?:|@import\s+["']https?:/)
  if (external) {
    throw new Error(`app ${app}: dist/${app}.html references an external resource: ${external[0]}`)
  }
  console.error(`built dist/${app}.html (${(html.length / 1024).toFixed(1)} KiB)`)
}
