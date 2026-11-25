// Bundles the host into dist/host.cjs, which `node` runs in development and which becomes the
// single-file sidecar exe later (Node SEA, see spike/RESULTS.md).
import { readFileSync } from 'node:fs'
import { build } from 'esbuild'

const sdk = JSON.parse(readFileSync('node_modules/@anthropic-ai/claude-agent-sdk/package.json', 'utf8'))

await build({
  entryPoints: ['src/main.ts'],
  bundle: true,
  platform: 'node',
  format: 'cjs',
  target: 'node22',
  outfile: 'dist/host.cjs',
  logLevel: 'warning',
  // The SDK is ESM and reads import.meta.url; CommonJS has none
  define: { 'import.meta.url': '__importMetaUrl', __SDK_VERSION__: JSON.stringify(sdk.version) },
  banner: { js: "const __importMetaUrl=require('url').pathToFileURL(__filename).href;" },
})
console.log(`dist/host.cjs built with Agent SDK ${sdk.version}`)
