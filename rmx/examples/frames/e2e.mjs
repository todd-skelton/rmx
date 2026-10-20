// Browser checks for the frames demo. Start the server first (`cargo run -p frames-demo`),
// then: node examples/frames/e2e.mjs
// Env: BASE (http://127.0.0.1:44110), PLAYWRIGHT_NODE_MODULES, CHROMIUM_PATH.
import { createRequire } from 'node:module'

const NODE_MODULES =
  process.env.PLAYWRIGHT_NODE_MODULES ??
  'C:/Users/ToddS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/'
const { chromium } = createRequire(NODE_MODULES)('playwright')
const BASE = process.env.BASE ?? 'http://127.0.0.1:44110'

const browser = await chromium.launch({
  executablePath:
    process.env.CHROMIUM_PATH ?? `${process.env.LOCALAPPDATA}/ms-playwright/chromium-1223/chrome-win64/chrome.exe`,
})
let failures = 0

function check(name, ok, detail) {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ' ' + JSON.stringify(detail)}`)
  if (!ok) failures++
}

async function open(path) {
  const page = await browser.newPage()
  page.errors = []
  page.on('pageerror', (e) => page.errors.push(e.message))
  page.on('response', (r) => r.status() >= 400 && !r.url().endsWith('/favicon.ico') && page.errors.push(`${r.status()} ${r.url()}`))
  page.on('console', (m) => m.type() !== 'log' && !m.text().startsWith('Failed to load resource') && page.errors.push(`${m.type()}: ${m.text()}`))
  await page.goto(BASE + path)
  await page.waitForFunction(() => document.documentElement.dataset.framesReady === 'true')
  return page
}

const text = (page) => page.evaluate(() => document.body.innerText)

// Home: frames stream in, counters hydrate, soft navigation keeps the JS context.
{
  const page = await open('/')
  await page.waitForSelector('.server-time', { timeout: 10000 })
  const body = await text(page)
  check('home: sidebar and nested activity frames resolved', body.includes('This content is rendered by') && body.includes('Nested frame with a hydrated counter'), body)
  await page.locator('main button:has-text("+")').first().click()
  check('home: root counter hydrated', (await page.locator('main span').first().innerText()) === '1')
  await page.click('#toggle-client-frame')
  await page.waitForFunction(() => document.body.innerText.includes('Nested server fragment'), null, { timeout: 5000 })
  check('home: client-mounted frame with nested frame', (await page.locator('main section').innerText()).includes('Inside mounted frame'))
  await page.evaluate(() => (window.__kept = true))
  await page.click('a[href="/time"]')
  await page.waitForFunction(() => document.querySelector('h1')?.innerText.startsWith('Server time'), null, { timeout: 10000 })
  check('nav: link is a soft navigation', await page.evaluate(() => window.__kept === true && location.pathname === '/time' && document.title === 'Server time'))
  await page.goBack()
  await page.waitForFunction(() => document.querySelector('h1')?.innerText === 'Full-stack Frames', null, { timeout: 10000 })
  check('nav: back button restores the page softly', await page.evaluate(() => window.__kept === true))
  check('home: no console errors', page.errors.length === 0, page.errors)
  await page.close()
}

// Time: frame.reload() from a client entry inside the frame, with reloadStart/Complete.
{
  const page = await open('/time')
  await page.waitForSelector('.server-time', { timeout: 10000 })
  const before = await page.locator('.server-time').innerText()
  await page.waitForTimeout(1100)
  await page.click('button.reload-time')
  await page.waitForTimeout(100)
  check('time: pending label during reload', (await page.locator('button.reload-time').innerText()).startsWith('Refreshing'))
  await page.waitForFunction((t) => document.querySelector('.server-time')?.innerText !== t, before, { timeout: 5000 })
  await page.waitForTimeout(100)
  check('time: label resets after reload', (await page.locator('button.reload-time').innerText()) === 'Refresh')
  await page.close()
}

// Reload scope: frame reload leaves the page alone; top reload keeps client entry state.
{
  const page = await open('/reload-scope')
  await page.waitForSelector('#non-blocking .frame-time', { timeout: 10000 })
  const state = () => page.evaluate(() => ({ page: document.querySelector('#page-time').innerText, nb: document.querySelector('#non-blocking .frame-time')?.innerText, root: document.querySelector('span').innerText }))
  for (let i = 0; i < 3; i++) await page.locator('button:has-text("+")').first().click()
  await page.waitForTimeout(1100)
  const s0 = await state()
  await page.click('#non-blocking button.reload-frame')
  await page.waitForFunction((t) => document.querySelector('#non-blocking .frame-time').innerText !== t, s0.nb, { timeout: 5000 })
  const s1 = await state()
  check('reload-scope: frame reload only updates the frame', s1.page === s0.page && s1.root === '3', { s0, s1 })
  await page.waitForTimeout(1100)
  await page.click('#reload-top')
  await page.waitForFunction((t) => document.querySelector('#page-time').innerText !== t, s0.page, { timeout: 8000 })
  const s2 = await state()
  check('reload-scope: top reload keeps client entry state', s2.root === '3', s2)
  await page.close()
}

// State search: a frame whose src follows component state.
{
  const page = await open('/state-search')
  await page.fill('input[name=query]', 'carolina')
  await page.press('input[name=query]', 'Enter')
  await page.waitForSelector('.state-results', { timeout: 5000 })
  const r = await page.evaluate(() => [...document.querySelectorAll('.state-results li')].map((l) => l.innerText))
  check('state-search: results frame', r.join() === 'North Carolina,South Carolina', r)
  await page.fill('input[name=query]', 'zzz')
  await page.press('input[name=query]', 'Enter')
  await page.waitForFunction(() => document.body.innerText.includes('No states matched'), null, { timeout: 5000 })
  check('state-search: src change reloads the frame', true)
  await page.close()
}

// Client-mounted: a nested non-blocking frame streams inside a client-mounted frame.
{
  const page = await open('/client-mounted')
  await page.click('#toggle-mounted-frame')
  const t0 = Date.now()
  await page.waitForFunction(() => document.body.innerText.includes('Outer server fragment'), null, { timeout: 5000 })
  const outer = Date.now() - t0
  const fallback = (await text(page)).includes('Loading nested non-blocking frame')
  await page.waitForFunction(() => document.body.innerText.includes('Nested server fragment'), null, { timeout: 6000 })
  check('client-mounted: outer content before nested frame finishes', fallback && outer < 2000, { outer, fallback })
  await page.close()
}

await browser.close()
console.log(failures ? `${failures} failed` : 'all passed')
process.exit(failures ? 1 : 0)
