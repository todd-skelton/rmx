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
  const bodyClass = await page.evaluate(() => {
    const html = document.documentElement
    html.setAttribute('class', 'dark')
    html.setAttribute('data-theme', 'dark')
    html.setAttribute('lang', 'fr')
    html.setAttribute('data-page', 'initial')
    document.body.setAttribute('data-client', 'yes')
    return document.body.className
  })
  await page.click('#reload-top')
  await page.waitForFunction((t) => document.querySelector('#page-time').innerText !== t, s0.page, { timeout: 8000 })
  const s2 = await state()
  check('reload-scope: top reload keeps client entry state', s2.root === '3', s2)
  const attrs = await page.evaluate(() => {
    const html = document.documentElement
    return { cls: html.getAttribute('class'), theme: html.getAttribute('data-theme'), lang: html.lang, page: html.hasAttribute('data-page'), client: document.body.hasAttribute('data-client'), body: document.body.className }
  })
  check(
    'reload-scope: top reload keeps preserved html attributes and reconciles the rest',
    attrs.cls === 'dark' && attrs.theme === 'dark' && attrs.lang === 'en' && !attrs.page && !attrs.client && attrs.body === bodyClass,
    attrs,
  )
  await page.waitForTimeout(1100)
  const before = await page.evaluate(() => {
    document.documentElement.removeAttribute('class')
    return document.querySelector('#page-time').innerText
  })
  await page.click('#reload-top')
  await page.waitForFunction((t) => document.querySelector('#page-time').innerText !== t, before, { timeout: 8000 })
  const absent = await page.evaluate(() => ({ cls: document.documentElement.hasAttribute('class'), theme: document.documentElement.getAttribute('data-theme') }))
  check('reload-scope: a removed preserved attribute stays absent across a top reload', !absent.cls && absent.theme === 'dark', absent)
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

async function until(page, fn, arg, timeout = 6000) {
  try {
    await page.waitForFunction(fn, arg, { timeout })
    return true
  } catch {
    return false
  }
}

const near = ({ expected, tolerance }) => Math.abs(window.scrollY - expected) < tolerance
const settled = () => window.navigation.transition === null
const hydration = (page, sel, n) => page.locator(sel).getByRole('button', { name: `Hydration check: ${n}`, exact: true })

// Scroll restoration: traversal restores scroll after reconciliation shrinks the document
// (scroll-restoration.test.e2e.ts, first case).
{
  const page = await open('/scroll-restoration')
  const repro = '#store-scroll-reproduction'
  await hydration(page, repro, 0).click()
  await hydration(page, repro, 1).waitFor()
  await page.locator('#scroll-restoration-list-end').scrollIntoViewIfNeeded()
  const listY = await page.evaluate(() => window.scrollY)
  await page.getByRole('link', { name: 'Open the shorter detail page' }).click()
  await page.getByRole('heading', { name: 'Short detail view' }).waitFor()
  check('scroll-restoration: hydration state kept on detail', await hydration(page, repro, 1).isVisible())
  await page.evaluate(() => window.scrollTo(0, 500))
  const detailY = await page.evaluate(() => window.scrollY)
  check('scroll-restoration: detail page scrolls', detailY > 100, detailY)
  await page.goBack()
  await page.getByText('List row 48', { exact: true }).waitFor()
  const restored = await until(page, near, { expected: listY, tolerance: 50 })
  check('scroll-restoration: Back restores list scroll', restored, { listY, now: await page.evaluate(() => window.scrollY) })
  check('scroll-restoration: hydration count still 1 after Back', await hydration(page, repro, 1).isVisible())
  check('scroll-restoration: no console errors', page.errors.length === 0, page.errors)
  await page.close()
}

// Scroll restoration: forward traversal after replace with resetScroll=false (second case).
{
  const page = await open('/scroll-restoration')
  const repro = '#store-scroll-reproduction'
  await hydration(page, repro, 0).click()
  await hydration(page, repro, 1).waitFor()
  await page.locator('#scroll-restoration-list-end').scrollIntoViewIfNeeded()
  const listY = await page.evaluate(() => window.scrollY)
  check('scroll-restoration fwd: list scrolls far', listY > 1000, listY)
  await page.getByRole('link', { name: 'Open the shorter detail page' }).click()
  await page.getByRole('heading', { name: 'Short detail view' }).waitFor()
  await until(page, settled)
  await page.evaluate(() => window.scrollTo(0, 150))
  const detailY = await page.evaluate(() => window.scrollY)
  await page.getByRole('button', { name: 'Select size without resetting scroll' }).click()
  await page.waitForURL((url) => url.searchParams.get('size') === 'small')
  await until(page, settled)
  const after = await page.evaluate(() => ({ y: window.scrollY, reset: window.navigation.currentEntry?.getState()?.resetScroll }))
  check('scroll-restoration fwd: replace keeps scroll and stores manual reset', after.y === detailY && after.reset === 'manual', { detailY, after })
  await page.goBack()
  await page.getByText('List row 48', { exact: true }).waitFor()
  check('scroll-restoration fwd: Back restores list', await until(page, near, { expected: listY, tolerance: 50 }), { listY, now: await page.evaluate(() => window.scrollY) })
  await page.goForward()
  await page.getByRole('heading', { name: 'Short detail view' }).waitFor()
  const fwd = await until(page, (a) => window.navigation.transition === null && Math.abs(window.scrollY - a.expected) < a.tolerance, { expected: detailY, tolerance: 50 })
  check('scroll-restoration fwd: Forward restores detail scroll', fwd, { detailY, now: await page.evaluate(() => window.scrollY) })
  check('scroll-restoration fwd: hydration count still 1', await hydration(page, repro, 1).isVisible())
  await page.close()
}

// Newsletter form: push and replace submissions keep scroll and show the status.
{
  const page = await open('/scroll-restoration')
  for (const mode of ['push', 'replace']) {
    const lengthBefore = await page.evaluate(() => window.navigation.entries().length)
    await page.locator('#newsletter-email').scrollIntoViewIfNeeded()
    const y = await page.evaluate(() => window.scrollY)
    await page.fill('#newsletter-email', 'todd@example.com')
    await page.getByRole('button', { name: `Subscribe with ${mode}` }).click()
    const shown = await until(page, (m) => document.querySelector('[role=status]')?.innerText.includes(m), mode)
    await until(page, settled)
    const state = await page.evaluate(() => ({ y: window.scrollY, len: window.navigation.entries().length, search: location.search }))
    const expectedLen = mode === 'push' ? lengthBefore + 1 : lengthBefore
    check(`newsletter ${mode}: status shown, scroll kept, history ${mode}`, shown && Math.abs(state.y - y) < 50 && state.len === expectedLen && state.search === `?newsletter=${mode}`, { y, state, lengthBefore })
  }
  check('newsletter: no console errors', page.errors.length === 0, page.errors)
  await page.close()
}

// Scroll anchoring: Back restores scroll after an earlier client entry is removed.
{
  const page = await open('/scroll-anchoring')
  const repro = '#scroll-anchoring-reproduction'
  await hydration(page, repro, 0).click()
  await hydration(page, repro, 1).waitFor()
  await page.locator('#scroll-anchoring-list-end').scrollIntoViewIfNeeded()
  const listY = await page.evaluate(() => window.scrollY)
  await page.getByRole('link', { name: 'Open the anchoring detail page' }).click()
  await page.getByRole('heading', { name: 'Scroll anchoring detail' }).waitFor()
  check('scroll-anchoring: hydration state kept on detail', await hydration(page, repro, 1).isVisible())
  await page.evaluate(() => window.scrollTo(0, 500))
  const detailY = await page.evaluate(() => window.scrollY)
  check('scroll-anchoring: detail page scrolls', detailY > 100, detailY)
  await page.goBack()
  await page.getByText('Anchoring row 48', { exact: true }).waitFor()
  check('scroll-anchoring: Back restores list scroll', await until(page, near, { expected: listY, tolerance: 50 }), { listY, now: await page.evaluate(() => window.scrollY) })
  check('scroll-anchoring: hydration count still 1', await hydration(page, repro, 1).isVisible())
  await page.close()
}

// Root reload: persistent entries keep setup and state with fresh props; removed entries dispose once.
{
  const page = await open('/root-reload-client-entries')
  await until(page, () => document.querySelector('#persistent-entry .setup-id')?.innerText !== 'hydrating')
  const read = () => page.evaluate(() => {
    const q = (s) => document.querySelector(s)?.innerText
    return { setup: q('#persistent-entry .setup-id'), count: q('#persistent-entry .local-count'), props: q('#persistent-entry .entry-server-props'), controls: q('#root-reload-controls .server-props'), removable: !!document.querySelector('#removable-entry'), disposed: q('.dispose-events'), state: q('.removable-state') }
  })
  await page.locator('#persistent-entry button.increment').click()
  await page.locator('#persistent-entry button.increment').click()
  const s0 = await read()
  await page.click('#reload-with')
  const pending = await until(page, () => document.querySelector('#reload-with')?.innerText.startsWith('Reloading with'), null, 900)
  await until(page, (v) => document.querySelector('#persistent-entry .entry-server-props')?.innerText !== v, s0.props, 6000)
  await until(page, () => document.querySelector('#reload-with')?.innerText === 'Reload with removable entry')
  const s1 = await read()
  check('root-reload: pending label while reloading', pending)
  check('root-reload: persistent entry keeps setup and count with new props', s1.setup === s0.setup && s1.count === '2' && s1.props !== s0.props && s1.controls === s1.props, { s0, s1 })
  await page.click('#reload-without')
  await until(page, () => !document.querySelector('#removable-entry'), null, 6000)
  await until(page, () => document.querySelector('.dispose-events')?.innerText === '1', null, 3000)
  const s2 = await read()
  const replacement = await page.locator('#removed-entry-replacement').count()
  check('root-reload: reload without entry disposes it once', !s2.removable && s2.disposed === '1' && s2.state === 'absent' && replacement === 1, { s2, replacement })
  check('root-reload: persistent entry survives removal reload', s2.setup === s0.setup && s2.count === '2', s2)
  check('root-reload: no console errors', page.errors.length === 0, page.errors)
  await page.close()
}

await browser.close()
console.log(failures ? `${failures} failed` : 'all passed')
process.exit(failures ? 1 : 0)
