// Browser checks for the spa demo, mirroring Remix's app.test.e2e.ts. Start the static
// server first (`cargo run -p spa-demo`), then: node demos/spa/e2e.mjs
// Env: BASE (http://127.0.0.1:44103), PLAYWRIGHT_NODE_MODULES, CHROMIUM_PATH.
import { createRequire } from 'node:module'

const NODE_MODULES =
  process.env.PLAYWRIGHT_NODE_MODULES ??
  'C:/Users/ToddS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/'
const { chromium } = createRequire(NODE_MODULES)('playwright')
const BASE = process.env.BASE ?? 'http://127.0.0.1:44103'

const browser = await chromium.launch({
  executablePath:
    process.env.CHROMIUM_PATH ?? `${process.env.LOCALAPPDATA}/ms-playwright/chromium-1223/chrome-win64/chrome.exe`,
})
let failures = 0
const check = (name, ok, detail) => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ' ' + JSON.stringify(detail)}`)
  if (!ok) failures++
}
async function fresh() {
  const page = await browser.newPage()
  page.errors = []
  page.logs = []
  page.on('pageerror', (e) => page.errors.push(e.message))
  page.on('console', (m) => {
    if (m.type() === 'warning' || m.type() === 'error') page.errors.push(m.text())
    else page.logs.push(m.text())
  })
  return page
}
const heading = (page, name) => page.getByRole('heading', { name }).waitFor({ timeout: 5000 })
const documentRequests = (page) => {
  const docs = []
  page.on('request', (r) => r.resourceType() === 'document' && docs.push(r.url()))
  return docs
}

// Deep link: the static server returns index.html, the router renders /about.
{
  const page = await fresh()
  await page.goto(BASE + '/about')
  await page.getByRole('status').waitFor({ timeout: 5000 })
  check('deep link shows the fallback first', true)
  await heading(page, 'URLs in, rendered UI out')
  check('deep link renders the route', new URL(page.url()).pathname === '/about')
  check('nav marks the current page', (await page.getByRole('link', { name: 'About' }).getAttribute('aria-current')) === 'page')
  check('router middleware logs requests', page.logs.some((l) => l.startsWith('[SPA] \u2190 200 GET /about')), page.logs)
  check('deep link: no page errors', page.errors.length === 0, page.errors)
  await page.close()
}

// Links navigate in the page; a newer navigation cancels a slower one.
{
  const page = await fresh()
  await page.goto(BASE + '/')
  await heading(page, 'A client-only rmx app')
  const docs = documentRequests(page)
  void page.getByRole('link', { name: 'About' }).click()
  await page.getByRole('status').waitFor({ timeout: 5000 })
  check('a link shows the loading indicator', true)
  await page.getByRole('link', { name: 'Home' }).click()
  await heading(page, 'A client-only rmx app')
  await page.waitForTimeout(900)
  check('the superseded About load never renders', (await page.getByRole('heading', { name: 'URLs in, rendered UI out' }).count()) === 0)
  check('the URL follows the last navigation', new URL(page.url()).pathname === '/')
  check('no document requests while navigating', docs.length === 0, docs)
  await page.goto(BASE + '/missing')
  await heading(page, 'Page not found')
  check('unknown URLs render the 404 page', true)
  check('links: no page errors', page.errors.length === 0, page.errors)
  await page.close()
}

// Forms: a new destination pushes, a submission to the current URL replaces.
{
  const page = await fresh()
  await page.goto(BASE + '/')
  await heading(page, 'A client-only rmx app')
  const startLength = await page.evaluate(() => history.length)
  await page.getByLabel('What should we call you?').fill('Ada')
  await page.getByRole('button', { name: 'Submit' }).click()
  await page.getByRole('status').waitFor({ timeout: 5000 })
  await heading(page, 'Hello, Ada!')
  check('form post renders the result', new URL(page.url()).pathname === '/greet')
  check('the first submission pushes an entry', (await page.evaluate(() => history.length)) === startLength + 1)

  await page.getByLabel('Try another name').fill('Grace')
  await page.getByRole('button', { name: 'Submit again' }).click()
  const pending = page.getByRole('button', { name: 'Submitting\u2026' })
  await pending.waitFor({ timeout: 5000 })
  check('the page shows its own pending state', (await pending.getAttribute('disabled')) === '')
  check('no loading indicator for the same URL', (await page.getByRole('status').count()) === 0)
  check('the old content stays while pending', (await page.getByRole('heading', { name: 'Hello, Ada!' }).count()) === 1)
  await heading(page, 'Hello, Grace!')
  const ready = page.getByRole('button', { name: 'Submit again' })
  await ready.waitFor({ timeout: 5000 })
  check('the button re-enables after the load', (await ready.getAttribute('disabled')) === null)
  check('a submission to the current URL replaces the entry', (await page.evaluate(() => history.length)) === startLength + 1)

  await page.goBack()
  await heading(page, 'A client-only rmx app')
  check('back returns home', new URL(page.url()).pathname === '/')
  await page.goForward()
  await heading(page, 'Hello, friend!')
  check('forward revisits /greet with GET', new URL(page.url()).pathname === '/greet')
  check('forms: no page errors', page.errors.length === 0, page.errors)
  await page.close()
}

await browser.close()
console.log(failures ? `${failures} failed` : 'all passed')
process.exit(failures ? 1 : 0)
