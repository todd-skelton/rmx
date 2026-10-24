// Browser checks for the lazy-frames demo. Start the server first
// (`cargo run -p lazy-frames-demo`), then: node demos/lazy-frames/e2e.mjs
// Env: BASE (http://127.0.0.1:44100), PLAYWRIGHT_NODE_MODULES, CHROMIUM_PATH.
import { createRequire } from 'node:module'

const NODE_MODULES =
  process.env.PLAYWRIGHT_NODE_MODULES ??
  'C:/Users/ToddS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/'
const { chromium } = createRequire(NODE_MODULES)('playwright')
const BASE = process.env.BASE ?? 'http://127.0.0.1:44100'

const browser = await chromium.launch({
  executablePath:
    process.env.CHROMIUM_PATH ?? `${process.env.LOCALAPPDATA}/ms-playwright/chromium-1223/chrome-win64/chrome.exe`,
})
const context = await browser.newContext({ viewport: { width: 1280, height: 800 } })
const page = await context.newPage()
const errors = []
page.on('pageerror', (e) => errors.push(e.message))
page.on('console', (m) => m.type() === 'warning' && m.text().startsWith('rsmix') && errors.push(m.text()))
const frameRequests = []
page.on('request', (r) => {
  const url = new URL(r.url())
  if (url.pathname.startsWith('/frames/')) frameRequests.push(url.pathname)
})
const requested = (path) => frameRequests.filter((p) => p === path).length
let failures = 0
const check = (name, ok, detail) => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ' ' + JSON.stringify(detail)}`)
  if (!ok) failures++
}
const ready = () => page.waitForFunction(() => document.documentElement.dataset.rmxReady === 'true', null, { timeout: 10000 })
const until = (fn, arg) => page.waitForFunction(fn, arg, { timeout: 5000 })
const sectionText = (id) => page.evaluate((id) => document.getElementById(id)?.innerText ?? '', id)

await context.clearCookies()
await page.goto(BASE + '/')
await ready()
await page.waitForTimeout(300)

// The regular Frame ships with the document; LazyFrames start as placeholders.
const artifacts = await page.evaluate(() => document.querySelectorAll('[data-delivery="eager"] [data-motion-artifact]').length)
check('eager Frame is part of the initial document', artifacts === 1, artifacts)
check('far sections make no request on load', requested('/frames/interactive/retained-counter') === 0 && requested('/frames/html/reading-list') === 0, frameRequests)
check('placeholders render with the page', (await sectionText('retained-counter')).includes('Not requested'))

// The lazy copy of the artifact loads when its card approaches.
await page.evaluate(() => document.querySelector('[data-delivery="lazy"]').scrollIntoView())
await until(() => document.querySelectorAll('[data-delivery="lazy"] [data-motion-artifact]').length === 1)
check('the lazy artifact loads near the viewport', requested('/frames/html/edition-orbit') === 1, frameRequests)

// Scrolling a section near the viewport mounts its Frame, once; the client entry inside hydrates.
await page.evaluate(() => document.getElementById('idea-counter').scrollIntoView())
await until(() => document.getElementById('idea-counter')?.innerText.includes('Client component ready'))
check('approaching a section loads its Frame', requested('/frames/interactive/idea-counter') === 1, frameRequests)

await page.click('#idea-counter button[data-action="increment"]')
await until(() => document.getElementById('idea-counter').innerText.includes('Local count: 13'))
check('client entry inside the frame responds', true)

// frame.reload() refetches the Frame and keeps local state.
await page.click('#idea-counter button[data-action="reload"]')
await until(() => performance.getEntriesByType('resource').filter((e) => e.name.endsWith('/frames/interactive/idea-counter')).length >= 2)
await page.waitForTimeout(200)
check('Reload Frame refetches the frame', requested('/frames/interactive/idea-counter') === 2, frameRequests)
check('local count survives the frame reload', (await sectionText('idea-counter')).includes('Local count: 13'))

// Leaving and coming back does not unmount or refetch.
await page.evaluate(() => window.scrollTo(0, 0))
await page.waitForTimeout(200)
await page.evaluate(() => document.getElementById('idea-counter').scrollIntoView())
await page.waitForTimeout(300)
check('scrolling back makes no request', requested('/frames/interactive/idea-counter') === 2, frameRequests)
check('scrolling back keeps the client state', (await sectionText('idea-counter')).includes('Local count: 13'))

// Animations pause while the lazy motion host is off screen.
const playState = () =>
  page.evaluate(() => {
    const orbit = document.querySelector('[data-delivery="lazy"] .edition-orbit__orbit')
    return orbit ? getComputedStyle(orbit).animationPlayState : 'missing'
  })
check('off-screen motion is paused', (await playState()) === 'paused', await playState())
await page.evaluate(() => document.querySelector('[data-delivery="lazy"]').scrollIntoView())
await until(() => {
  const orbit = document.querySelector('[data-delivery="lazy"] .edition-orbit__orbit')
  return orbit && getComputedStyle(orbit).animationPlayState === 'running'
})
check('on-screen motion runs', true)
const eagerState = await page.evaluate(() => getComputedStyle(document.querySelector('[data-delivery="eager"] .edition-orbit__orbit')).animationPlayState)
check('the regular Frame is not affected', eagerState === 'running', eagerState)

// Theme: the whole document switches without refetching or remounting frames.
const before = frameRequests.length
await page.evaluate(() => (window.__keep = true))
await page.click('button[aria-label="Switch to dark mode"]')
await until(() => document.documentElement.dataset.theme === 'dark' && document.body.dataset.theme === 'dark')
check('theme toggle switches the document', true)
check('theme cookie is saved', (await context.cookies()).some((c) => c.name === 'lazy-frames-theme' && c.value === 'dark'))
check('theme change makes no frame requests', frameRequests.length === before, frameRequests.slice(before))
check('theme change keeps client state', (await sectionText('idea-counter')).includes('Local count: 13'))
const toggleLabel = await page.evaluate(() => document.querySelector('button[aria-label^="Switch to"]').getAttribute('aria-label'))
check('toggle offers the other theme', toggleLabel === 'Switch to light mode', toggleLabel)

// Latency: a form post sets a cookie and the next responses carry the delay.
await page.click('form[action="/demo-latency"] button[type="submit"]')
await until(() => document.querySelector('form[action="/demo-latency"] strong')?.innerText === 'Global latency on')
check('latency toggle navigates softly', await page.evaluate(() => window.__keep === true))
check('server renders the saved theme after navigation', await page.evaluate(() => document.documentElement.dataset.theme === 'dark'))
const timing = await page.evaluate(async () => (await fetch('/frames/html/field-notes')).headers.get('server-timing'))
check('latency applies to frame requests', timing === 'demo-latency;dur=520', timing)
await page.click('form[action="/demo-latency"] button[type="submit"]')
await until(() => document.querySelector('form[action="/demo-latency"] strong')?.innerText === 'Global latency off')

// Hash links scroll in place.
await page.evaluate(() => window.scrollTo(0, 0))
await page.click('a[href="#delivery-comparison"]')
await until(() => location.hash === '#delivery-comparison' && window.scrollY > 100)
check('hash link scrolls without a navigation', await page.evaluate(() => window.__keep === true))

// The last section loads at the end of the page.
await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight))
await until(() => document.getElementById('retained-counter')?.innerText.includes('Client component ready'))
check('last section hydrates near the end', requested('/frames/interactive/retained-counter') === 1, frameRequests)

const counts = {}
for (const p of frameRequests) counts[p] = (counts[p] ?? 0) + 1
check('every frame was requested at most once (plus the explicit reload)', Object.entries(counts).every(([p, n]) => n === 1 || (p.endsWith('idea-counter') && n === 2)), counts)
check('no page errors or rsmix warnings', errors.length === 0, errors)

await browser.close()
console.log(failures ? `${failures} failed` : 'all passed')
process.exit(failures ? 1 : 0)
