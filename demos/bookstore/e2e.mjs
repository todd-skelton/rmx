// Browser checks for the bookstore demo: the three client entries (CartButton, CartItems,
// ImageCarousel) plus the plain HTML flows that must work without JavaScript.
// Start the server first (`cargo run -p bookstore-demo`), then: node demos/bookstore/e2e.mjs
// Env: BASE (http://127.0.0.1:44108), PLAYWRIGHT_NODE_MODULES, CHROMIUM_PATH, SHOT (screenshot path).
import { createRequire } from 'node:module'

const NODE_MODULES =
  process.env.PLAYWRIGHT_NODE_MODULES ??
  'C:/Users/ToddS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/'
const { chromium } = createRequire(NODE_MODULES)('playwright')
const BASE = process.env.BASE ?? 'http://127.0.0.1:44108'

const browser = await chromium.launch({
  executablePath:
    process.env.CHROMIUM_PATH ?? `${process.env.LOCALAPPDATA}/ms-playwright/chromium-1223/chrome-win64/chrome.exe`,
})
let failures = 0
const check = (name, ok, detail) => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ' ' + JSON.stringify(detail)}`)
  if (!ok) failures++
}
const attempt = async (name, fn) => {
  try {
    await fn()
    check(name, true)
  } catch (e) {
    check(name, false, e.message.split('\n')[0])
  }
}
const ready = (page) => page.waitForFunction(() => document.documentElement.dataset.rmxReady === 'true', null, { timeout: 10000 })

// --- CartButton: toggles through the frame, showing a pending label ---------------------
{
  const context = await browser.newContext({ baseURL: BASE })
  const page = await context.newPage()
  const errors = []
  page.on('pageerror', (e) => errors.push(e.message))
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()))
  await page.goto('/books')
  await ready(page)
  const card = page.locator('.book-card').filter({ hasText: 'Ash & Smoke' })
  await attempt('cart button shows Saving... then Remove from Cart', async () => {
    await card.getByRole('button', { name: 'Add to Cart' }).click()
    await card.getByRole('button', { name: 'Saving...' }).waitFor({ timeout: 3000 })
    await card.getByRole('button', { name: 'Remove from Cart' }).waitFor({ timeout: 5000 })
  })
  await attempt('the cart page lists the toggled book', async () => {
    await page.goto('/cart')
    await page.locator('table').getByText('Ash & Smoke').waitFor({ timeout: 5000 })
  })
  await attempt('toggling again removes it', async () => {
    await page.goto('/books')
    await ready(page)
    await card.getByRole('button', { name: 'Remove from Cart' }).click()
    await card.getByRole('button', { name: 'Add to Cart' }).waitFor({ timeout: 5000 })
    await page.goto('/cart')
    await page.getByText('Your cart is empty.').waitFor({ timeout: 5000 })
  })
  check('no page errors on /books', errors.length === 0, errors)
  await context.close()
}

// --- ImageCarousel ---------------------------------------------------------------------
{
  const page = await browser.newPage({ baseURL: BASE })
  await page.goto('/books/bbq')
  await ready(page)
  const prev = page.getByRole('button', { name: 'Previous image' })
  const next = page.getByRole('button', { name: 'Next image' })
  const track = page.locator('img[alt="Image 1 of 3"]').locator('xpath=../..')
  await attempt('carousel starts on the first image with Previous disabled', async () => {
    if (!(await prev.isDisabled())) throw new Error('previous enabled')
    if (await next.isDisabled()) throw new Error('next disabled')
  })
  await attempt('Next slides the track and disables at the end', async () => {
    await next.click()
    await page.waitForFunction((el) => el.style.transform === 'translateX(-100%)', await track.elementHandle(), { timeout: 3000 })
    await next.click()
    await page.waitForFunction((el) => el.style.transform === 'translateX(-200%)', await track.elementHandle(), { timeout: 3000 })
    if (!(await next.isDisabled())) throw new Error('next still enabled')
    await prev.click()
    await page.waitForFunction((el) => el.style.transform === 'translateX(-100%)', await track.elementHandle(), { timeout: 3000 })
  })
  await page.close()
}

// --- CartItems: update and remove with pending UI, then the frame reloads ---------------
{
  const context = await browser.newContext({ baseURL: BASE })
  const page = await context.newPage()
  const errors = []
  page.on('pageerror', (e) => errors.push(e.message))
  await page.request.post('/cart/api/add', { form: { bookId: '1', slug: 'bbq' } })
  await page.request.post('/cart/api/add', { form: { bookId: '2', slug: 'heavy-metal' } })
  await page.goto('/cart')
  await ready(page)
  const origin = await page.evaluate(() => performance.timeOrigin)
  const row = page.locator('tr').filter({ hasText: 'Ash & Smoke' })
  await attempt('update shows Saving... and the new total', async () => {
    await row.locator('input[name=quantity]').fill('3')
    await row.getByRole('button', { name: 'Update' }).click()
    await row.getByRole('button', { name: 'Saving...' }).waitFor({ timeout: 3000 })
    await page.getByText('Updating your cart...').waitFor({ timeout: 3000 })
    await page.getByText('$77.97').waitFor({ timeout: 5000 })
  })
  await attempt('remove shows Removing... and drops the row', async () => {
    await page.locator('tr').filter({ hasText: 'Heavy Metal' }).getByRole('button', { name: 'Remove' }).click()
    await page.getByRole('button', { name: 'Removing...' }).waitFor({ timeout: 3000 })
    await page.locator('tr').filter({ hasText: 'Heavy Metal' }).waitFor({ state: 'detached', timeout: 5000 })
    await page.getByText('$50.97').first().waitFor({ timeout: 5000 })
  })
  check('cart edits stay in the same document', (await page.evaluate(() => performance.timeOrigin)) === origin)
  check('no page errors on /cart', errors.length === 0, errors)
  await context.close()
}

// --- Without JavaScript: forms, method override, login and checkout ---------------------
{
  const context = await browser.newContext({ baseURL: BASE, javaScriptEnabled: false })
  const page = await context.newPage()
  await page.request.post('/cart/api/add', { form: { bookId: '3', slug: 'three-ways' } })
  await attempt('no-JS cart update and remove post through method override', async () => {
    await page.goto('/cart')
    const row = page.locator('tr').filter({ hasText: 'Three Ways' })
    await row.locator('input[name=quantity]').fill('2')
    await row.getByRole('button', { name: 'Update' }).click()
    await page.getByText('$57.98').first().waitFor({ timeout: 5000 })
    await page.locator('tr').filter({ hasText: 'Three Ways' }).getByRole('button', { name: 'Remove' }).click()
    await page.getByText('Your cart is empty.').waitFor({ timeout: 5000 })
  })
  await attempt('no-JS login with returnTo lands on checkout', async () => {
    await page.goto('/checkout')
    if (!page.url().includes('/login?returnTo=%2Fcheckout')) throw new Error(page.url())
    await page.getByLabel('Email').fill('customer@example.com')
    await page.getByLabel('Password').fill('password123')
    await page.getByRole('button', { name: 'Login' }).click()
    await page.waitForURL('**/checkout', { timeout: 5000 })
  })
  await attempt('no-JS checkout creates an order', async () => {
    await page.request.post('/cart/api/add', { form: { bookId: '1', slug: 'bbq' } })
    await page.goto('/checkout')
    await page.getByLabel('Street Address').fill('1 Rust Way')
    await page.getByLabel('City').fill('Ferris')
    await page.getByLabel('State').fill('CR')
    await page.getByLabel('ZIP').fill('12345')
    await page.getByRole('button', { name: 'Place Order' }).click()
    await page.waitForURL('**/confirmation', { timeout: 5000 })
    await page.getByText('Order Confirmed').first().waitFor({ timeout: 5000 })
  })
  await context.close()
}

// --- Admin: create a book with an uploaded cover ----------------------------------------
{
  const context = await browser.newContext({ baseURL: BASE })
  const page = await context.newPage()
  const slug = `e2e-${Date.now()}`
  await attempt('admin uploads a cover that is served from /uploads', async () => {
    await page.goto('/login')
    await page.getByLabel('Email').fill('admin@bookstore.com')
    await page.getByLabel('Password').fill('admin123')
    await page.getByRole('button', { name: 'Login' }).click()
    await page.waitForURL('**/account')
    await page.goto('/admin/books/new')
    const fields = { title: `E2E ${slug}`, slug, author: 'Playwright', isbn: '978-1111111111', genre: 'testing', price: '9.99', publishedYear: '2026', description: 'Created by e2e.mjs' }
    for (const [name, value] of Object.entries(fields)) await page.locator(`[name=${name}]`).fill(value)
    await page.getByLabel('Book Cover Image').setInputFiles(new URL('./public/images/placeholder.jpg', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1'))
    const [post] = await Promise.all([
      page.waitForResponse((r) => r.request().method() === 'POST' && r.url().endsWith('/admin/books')),
      page.getByRole('button', { name: 'Create Book' }).click(),
    ])
    await page.waitForURL('**/admin/books', { timeout: 5000 })
    await page.goto('/books')
    const src = await page
      .locator(`img[alt="E2E ${slug}"]`)
      .getAttribute('src', { timeout: 5000 })
      .catch(() => `missing (POST ${post.status()}, landed on ${page.url()})`)
    if (!src?.startsWith('/uploads/cover/')) throw new Error(`cover src ${src}`)
    const res = await page.request.get(src)
    if (res.status() !== 200 || res.headers()['content-type'] !== 'image/jpeg') throw new Error(`${res.status()} ${res.headers()['content-type']}`)
  })
  if (process.env.SHOT) {
    await page.goto('/books')
    await page.screenshot({ path: process.env.SHOT, fullPage: true })
  }
  await context.close()
}

// --- Prefetching: intent on header links, viewport on "View Details" (Remix 3.1) ---------
{
  const context = await browser.newContext({ baseURL: BASE })
  const page = await context.newPage()
  const frameGets = []
  page.on('request', (r) => r.headers()['x-remix-frame'] === 'true' && r.method() === 'GET' && frameGets.push(new URL(r.url()).pathname))
  await page.goto('/books')
  await ready(page)
  await attempt('View Details links prefetch their book page once they scroll into view', async () => {
    await page.waitForTimeout(300)
    if (frameGets.some((p) => p.startsWith('/books/'))) throw new Error(`prefetched below the fold: ${JSON.stringify(frameGets)}`)
    await page.locator('a[data-rmx-prefetch="viewport"]').first().scrollIntoViewIfNeeded()
    for (let i = 0; i < 40 && !frameGets.some((p) => p.startsWith('/books/')); i++) await page.waitForTimeout(50)
    if (!frameGets.some((p) => p.startsWith('/books/'))) throw new Error(`frame GETs: ${JSON.stringify(frameGets)}`)
  })
  await attempt('hovering a header link prefetches it before the click', async () => {
    await page.getByRole('link', { name: 'About', exact: true }).hover()
    for (let i = 0; i < 40 && !frameGets.includes('/about'); i++) await page.waitForTimeout(50)
    if (!frameGets.includes('/about')) throw new Error(`frame GETs: ${JSON.stringify(frameGets)}`)
    await page.getByRole('link', { name: 'About', exact: true }).click()
    await page.waitForURL('**/about', { timeout: 5000 })
  })
  await context.close()
}

await browser.close()
console.log(failures ? `${failures} check(s) failed` : 'all checks passed')
process.exit(failures ? 1 : 0)
