// Makes the agent host a single exe for the installer (Node single executable application): the bundle from
// build.mjs, injected into a copy of the Node that runs this script. That Node becomes the shipped runtime, so
// the release pipeline pins its version. The exe lands where Tauri looks for the sidecar (`externalBin` in
// src-tauri/tauri.release.conf.json), named with the target triple as Tauri expects.
//
// It doesn't carry Claude Code: it runs the one installed on the computer (`--claude`), see spike/RESULTS.md.
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join } from 'node:path'

if (process.platform !== 'win32') throw new Error('The sidecar is built on Windows only')

execFileSync(process.execPath, ['build.mjs'], { stdio: 'inherit' })

const blob = 'dist/sea-prep.blob'
writeFileSync('dist/sea-config.json', JSON.stringify({ main: 'dist/host.cjs', output: blob, disableExperimentalSEAWarning: true }))
execFileSync(process.execPath, ['--experimental-sea-config', 'dist/sea-config.json'], { stdio: 'inherit' })

const out = '../src-tauri/binaries/sprocket-agent-host-x86_64-pc-windows-msvc.exe'
mkdirSync(dirname(out), { recursive: true })
rmSync(out, { force: true })
copyFileSync(process.execPath, out)

// Injecting the blob voids node.exe's Authenticode signature; the exe runs all the same (unsigned, like the app)
const postject = join(dirname(createRequire(import.meta.url).resolve('postject/package.json')), 'dist', 'cli.js')
execFileSync(
  process.execPath,
  [postject, out, 'NODE_SEA_BLOB', blob, '--sentinel-fuse', 'NODE_SEA_FUSE_fce680ab2cc467b6e072b8b5df1996b2', '--overwrite'],
  { stdio: 'inherit' },
)
console.log(`${out} built with Node ${process.version}`)
