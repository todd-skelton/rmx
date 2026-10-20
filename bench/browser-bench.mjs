// Browser benchmark for rmx (Rust/WASM) vs Remix 3 (JS). Both apps must be running:
//   remix: http://127.0.0.1:44100   rmx: http://127.0.0.1:44101
// Usage: node browser-bench.mjs [--iterations 15] [--loads 10] [--out results/browser.json]
//
// Everything is measured from outside the frameworks with the same code: click a DOM
// element, wait one macrotask (each framework flushes updates on a microtask), read the
// clock ("commit"), then wait for the next animation frame plus a macrotask ("frame",
// which includes style, layout and paint).

import { createRequire } from 'node:module'
import { writeFileSync } from 'node:fs'
import { gzipSync, brotliCompressSync } from 'node:zlib'

const NODE_MODULES =
  process.env.PLAYWRIGHT_NODE_MODULES ??
  'C:/Users/ToddS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/'
const { chromium } = createRequire(NODE_MODULES)('playwright')

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith('--') ? [...acc, [a.slice(2), all[i + 1]]] : acc), []),
)
const ITERATIONS = Number(args.iterations ?? 15)
const LOADS = Number(args.loads ?? 10)
const WARMUP = 3
const TARGETS = [
  { name: 'remix', url: 'http://127.0.0.1:44100' },
  { name: 'rmx', url: 'http://127.0.0.1:44101' },
]

const OPS = [
  { name: 'create 1k', prep: ['#clear'], click: '#create', expect: { rows: 1000 } },
  { name: 'replace 1k', prep: ['#create'], click: '#create', expect: { rows: 1000 } },
  { name: 'update every 10th', prep: ['#create'], click: '#update', expect: { rows: 1000, firstLabelEndsWith: ' !!!' } },
  { name: 'select row', prep: ['#create'], click: 'tbody tr:nth-child(2) a.select', expect: { rows: 1000, secondSelected: true } },
  { name: 'swap rows', prep: ['#create'], click: '#swap', expect: { rows: 1000 } },
  { name: 'remove row', prep: ['#create'], click: 'tbody tr:nth-child(4) a.remove', expect: { rows: 999 } },
  { name: 'create 10k', prep: ['#clear'], click: '#create10k', expect: { rows: 10000 }, iterations: 5 },
  { name: 'append 1k', prep: ['#create'], click: '#append', expect: { rows: 2000 } },
  { name: 'clear 1k', prep: ['#create'], click: '#clear', expect: { rows: 0 } },
]

async function clickAndMeasure(page, selector) {
  return page.evaluate(async (sel) => {
    const el = document.querySelector(sel)
    if (!el) throw new Error(`missing ${sel}`)
    const t0 = performance.now()
    el.click()
    await new Promise((r) => setTimeout(r, 0))
    const t1 = performance.now()
    const rowsAtCommit = document.querySelectorAll('tbody tr').length
    await new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0)))
    const t2 = performance.now()
    return { commit: t1 - t0, frame: t2 - t0, rowsAtCommit }
  }, selector)
}

async function state(page) {
  return page.evaluate(() => {
    const rows = document.querySelectorAll('tbody tr')
    return {
      rows: rows.length,
      firstLabel: rows[0]?.querySelector('a.select')?.textContent ?? null,
      secondSelected: rows[1]?.className === 'danger',
      lastOp: document.documentElement.dataset.lastOp ?? null,
    }
  })
}

function check(op, s, m) {
  const e = op.expect
  const problems = []
  if (s.rows !== e.rows) problems.push(`rows ${s.rows} != ${e.rows}`)
  if (m.rowsAtCommit !== e.rows) problems.push(`rows at commit ${m.rowsAtCommit} != ${e.rows}`)
  if (e.firstLabelEndsWith && !s.firstLabel?.endsWith(e.firstLabelEndsWith)) problems.push(`label ${s.firstLabel}`)
  if (e.secondSelected && !s.secondSelected) problems.push('row 2 not selected')
  if (problems.length) throw new Error(`${op.name}: ${problems.join(', ')}`)
}

const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b)
  const m = s.length >> 1
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2
}
const round = (x, d = 2) => Math.round(x * 10 ** d) / 10 ** d

async function loadMetrics(browser, target) {
  const runs = []
  let assets = null
  for (let i = 0; i < LOADS; i++) {
    const context = await browser.newContext() // fresh HTTP cache every load
    const page = await context.newPage()
    const bodies = []
    page.on('response', async (res) => {
      const type = res.request().resourceType()
      if (['script', 'fetch', 'other'].includes(type) || /\.(js|ts|tsx|wasm)(\?|$)/.test(res.url())) {
        try {
          bodies.push({ url: res.url(), body: await res.body() })
        } catch {}
      }
    })
    await page.goto(`${target.url}/client`, { waitUntil: 'load' })
    await page.waitForFunction(() => document.documentElement.dataset.hydrated, null, { timeout: 15000 })
    const timing = await page.evaluate(() => {
      const nav = performance.getEntriesByType('navigation')[0]
      return {
        domContentLoaded: nav.domContentLoadedEventEnd,
        load: nav.loadEventEnd,
        hydrated: Number(document.documentElement.dataset.hydrated),
      }
    })
    if (i === 0) {
      await page.waitForTimeout(250)
      const files = bodies.filter((b) => /\.(js|mjs|ts|tsx|wasm)(\?|$)/.test(new URL(b.url).pathname))
      const all = files.map((f) => f.body)
      const total = Buffer.concat(all)
      assets = {
        files: files.length,
        rawKB: round(total.length / 1024, 1),
        gzipKB: round(all.reduce((n, b) => n + gzipSync(b, { level: 9 }).length, 0) / 1024, 1),
        brotliKB: round(all.reduce((n, b) => n + brotliCompressSync(b).length, 0) / 1024, 1),
        list: files.map((f) => ({ path: new URL(f.url).pathname, bytes: f.body.length })),
      }
    }
    runs.push(timing)
    await context.close()
  }
  return {
    assets,
    hydratedMs: round(median(runs.map((r) => r.hydrated))),
    domContentLoadedMs: round(median(runs.map((r) => r.domContentLoaded))),
    loadMs: round(median(runs.map((r) => r.load))),
  }
}

async function counterCheck(browser, target) {
  const page = await browser.newPage()
  await page.goto(`${target.url}/`)
  await page.waitForFunction(() => document.querySelector('#counter'))
  // Retry until hydrated: clicks before hydration do nothing.
  const before = await page.textContent('#counter')
  let after = before
  for (let i = 0; i < 50 && after === before; i++) {
    await page.click('#counter')
    await page.waitForTimeout(50)
    after = await page.textContent('#counter')
  }
  await page.close()
  return { before, after, ok: before !== after }
}

async function opBenchmarks(browser, target) {
  const page = await browser.newPage()
  const errors = []
  page.on('pageerror', (e) => errors.push(String(e)))
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()))
  await page.goto(`${target.url}/client`)
  await page.waitForFunction(() => document.documentElement.dataset.hydrated)

  // Same seed and id sequence in both apps: the first created label must match.
  await clickAndMeasure(page, '#clear')
  await clickAndMeasure(page, '#create')
  const firstLabel = (await state(page)).firstLabel

  const results = {}
  for (const op of OPS) {
    const n = op.iterations ?? ITERATIONS
    const commit = []
    const frame = []
    const internal = []
    for (let i = 0; i < WARMUP + n; i++) {
      for (const p of op.prep) await clickAndMeasure(page, p)
      const m = await clickAndMeasure(page, op.click)
      const s = await state(page)
      check(op, s, m)
      if (i >= WARMUP) {
        commit.push(m.commit)
        frame.push(m.frame)
        const internalMs = s.lastOp && op.click.startsWith('#') ? Number(s.lastOp.split(':')[1]) : null
        if (internalMs != null) internal.push(internalMs)
      }
    }
    results[op.name] = {
      n,
      commitMs: round(median(commit)),
      frameMs: round(median(frame)),
      internalMs: internal.length ? round(median(internal)) : null,
    }
  }
  const cdp = await page.context().newCDPSession(page)
  await cdp.send('Performance.enable')
  const heap = await cdp.send('Performance.getMetrics').catch(() => null)
  await page.close()
  return { firstLabel, results, errors, jsHeapMB: heap ? round((heap.metrics.find((m) => m.name === 'JSHeapUsedSize')?.value ?? NaN) / 2 ** 20, 1) : null }
}

// The bundled Playwright expects a newer browser build than is installed; use the installed
// Playwright Chromium instead (override with CHROMIUM_PATH).
const browser = await chromium.launch({
  headless: true,
  executablePath:
    process.env.CHROMIUM_PATH ?? `${process.env.LOCALAPPDATA}/ms-playwright/chromium-1223/chrome-win64/chrome.exe`,
})
const out = { date: new Date().toISOString(), chromium: browser.version(), iterations: ITERATIONS, loads: LOADS, targets: {} }
for (const target of TARGETS) {
  console.error(`== ${target.name}`)
  const counter = await counterCheck(browser, target)
  const load = await loadMetrics(browser, target)
  const ops = await opBenchmarks(browser, target)
  out.targets[target.name] = { counter, load, ...ops }
}
await browser.close()

writeFileSync(args.out ?? 'results/browser.json', JSON.stringify(out, null, 2))

const [a, b] = TARGETS.map((t) => out.targets[t.name])
console.log(`chromium ${out.chromium}; medians; commit = click to DOM updated, frame = to next painted frame`)
console.log(`counter hydrated+clickable: remix ${a.counter.ok} (${a.counter.before} -> ${a.counter.after}), rmx ${b.counter.ok} (${b.counter.before} -> ${b.counter.after})`)
console.log(`first label: remix "${a.firstLabel}", rmx "${b.firstLabel}"`)
console.log(`client assets: remix ${a.load.assets.files} files ${a.load.assets.rawKB} KB raw / ${a.load.assets.gzipKB} KB gzip / ${a.load.assets.brotliKB} KB br; rmx ${b.load.assets.files} files ${b.load.assets.rawKB} KB raw / ${b.load.assets.gzipKB} KB gzip / ${b.load.assets.brotliKB} KB br`)
console.log(`hydrated at (ms since navigation): remix ${a.load.hydratedMs}, rmx ${b.load.hydratedMs}; DCL remix ${a.load.domContentLoadedMs}, rmx ${b.load.domContentLoadedMs}`)
console.log('op'.padEnd(20), 'remix commit', 'rmx commit', '| remix frame', 'rmx frame')
for (const op of OPS) {
  const r = a.results[op.name]
  const s = b.results[op.name]
  console.log(op.name.padEnd(20), String(r.commitMs).padStart(12), String(s.commitMs).padStart(10), '|', String(r.frameMs).padStart(12), String(s.frameMs).padStart(9))
}
console.log(`page errors: remix ${a.errors.length}, rmx ${b.errors.length}; JS heap MB after run: remix ${a.jsHeapMB}, rmx ${b.jsHeapMB} (excludes WASM linear memory)`)
