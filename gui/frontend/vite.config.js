import { defineConfig, loadEnv } from 'vite'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// https://vite.dev/config/
const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), '')
  const ngrokHost = env.VITE_DEV_HOST || 'timbered-greedily-alyvia.ngrok-free.dev'
  return {
    plugins: [svelte()],
    clearScreen: false,
    server: {
      strictPort: true,
      host: env.TAURI_DEV_HOST || '0.0.0.0',
      port: 5173,
      allowedHosts: ['localtest.me', '127.0.0.1', 'localhost', ngrokHost],
      https: env.VITE_HTTPS_KEY && env.VITE_HTTPS_CERT
        ? {
            key: fs.readFileSync(env.VITE_HTTPS_KEY),
            cert: fs.readFileSync(env.VITE_HTTPS_CERT),
          }
        : undefined,
      hmr: {
        protocol: 'wss',
        host: ngrokHost,
        clientPort: 443,
      },
    },
  }
})
