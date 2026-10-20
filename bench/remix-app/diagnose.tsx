// Diagnostic: where does Remix spend time per request? Calls router.fetch directly (no HTTP)
// and renderToString on the page component, so middleware and rendering can be separated.
import { renderToString } from 'remix/component/server'
import { router } from './app/router.tsx'
import { SsrPage } from './app/pages.tsx'
import { assets } from './app/assets.ts'
import { pathToFileURL } from 'node:url'
import * as path from 'node:path'

let counterUrl = pathToFileURL(path.resolve('app/public/counter.tsx')).href

async function time(label, n, f) {
  for (let i = 0; i < Math.min(20, n); i++) await f()
  let t0 = performance.now()
  for (let i = 0; i < n; i++) await f()
  let ms = (performance.now() - t0) / n
  console.log(`${label.padEnd(40)} ${ms.toFixed(3)} ms/op`)
}

let drain = async (res) => { await res.arrayBuffer() }
console.log(`NODE_ENV=${process.env.NODE_ENV}`)
await time('router.fetch /app.css (static file)', 200, async () => drain(await router.fetch('http://x/app.css')))
await time('router.fetch / (5 tasks + 1 entry)', 200, async () => drain(await router.fetch('http://x/')))
await time('router.fetch /ssr/0', 200, async () => drain(await router.fetch('http://x/ssr/0')))
await time('router.fetch /ssr/100', 100, async () => drain(await router.fetch('http://x/ssr/100')))
await time('router.fetch /ssr/1000', 20, async () => drain(await router.fetch('http://x/ssr/1000')))
await time('assets.getScriptEntry(counter.tsx)', 200, () => assets.getScriptEntry(counterUrl))
await time('renderToString(<SsrPage n=0/>)', 200, () => renderToString(<SsrPage n={0} />))
await time('renderToString(<SsrPage n=100/>)', 100, () => renderToString(<SsrPage n={100} />))
await time('renderToString(<SsrPage n=1000/>)', 20, () => renderToString(<SsrPage n={1000} />))
