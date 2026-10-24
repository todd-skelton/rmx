// Browser checks for the i18n demo, ported from app/app.test.e2e.ts. Start the server first
// (`cargo run -p i18n-demo`), then: node demos/i18n/e2e.mjs
// Env: BASE (http://127.0.0.1:44107), PLAYWRIGHT_NODE_MODULES, CHROMIUM_PATH, SHOT (screenshot path).
import { createRequire } from 'node:module'

const NODE_MODULES =
  process.env.PLAYWRIGHT_NODE_MODULES ??
  'C:/Users/ToddS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/'
const { chromium } = createRequire(NODE_MODULES)('playwright')
const BASE = process.env.BASE ?? 'http://127.0.0.1:44107'

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

// --- Full-document language navigations, then save and clear preferences ---------------
{
  const context = await browser.newContext({ baseURL: BASE, extraHTTPHeaders: { 'Accept-Language': 'en-US,en;q=0.9' } })
  const page = await context.newPage()
  const errors = []
  page.on('pageerror', (e) => errors.push(e.message))
  page.on('console', (m) => m.type() === 'warning' && m.text().startsWith('rsmix') && errors.push(m.text()))
  const origin = () => page.evaluate(() => performance.timeOrigin)

  await page.goto('/en')
  const cartQuantity = page.getByRole('spinbutton', { name: 'Cart quantity' })
  const cartSummary = page.getByRole('status', { name: 'Cart summary' })
  await attempt('the cart starts empty and counts typed quantities', async () => {
    await cartSummary.filter({ hasText: 'Your cart is empty' }).waitFor({ timeout: 5000 })
    await cartQuantity.fill('2')
    await cartSummary.filter({ hasText: 'You have 2 items in your cart' }).waitFor({ timeout: 5000 })
  })
  await attempt('the browser-formatted value increases', async () => {
    await page.getByRole('button', { name: 'Increase value', exact: true }).click()
    await page.getByRole('status').filter({ hasText: '1,251,000' }).waitFor({ timeout: 5000 })
  })
  const documentStarted = await origin()

  await page.getByRole('link', { name: 'العربية', exact: true }).click()
  await attempt('an Arabic link loads a right-to-left document', () => page.locator('html[lang="ar"][dir="rtl"]').waitFor({ timeout: 5000 }))
  check('the Arabic URL is /ar', new URL(page.url()).pathname === '/ar', page.url())
  check('the Arabic title', (await page.title()) === 'مثال التدويل في Remix', await page.title())
  check('the switcher shows ar', (await page.getByRole('combobox').inputValue()) === 'ar')
  await page.setViewportSize({ width: 375, height: 812 })
  check(
    'no horizontal overflow at 375px',
    await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth),
  )
  const arabicDocumentStarted = await origin()
  check('language links are full document navigations', arabicDocumentStarted !== documentStarted)

  const arabicCartQuantity = page.getByRole('spinbutton', { name: 'كمية سلة التسوق' })
  const arabicCartSummary = page.getByRole('status', { name: 'ملخص سلة التسوق' })
  check('a new document starts a new cart', (await arabicCartQuantity.inputValue()) === '0')
  await attempt('Arabic dual and few plural forms', async () => {
    await arabicCartSummary.filter({ hasText: 'سلة التسوق فارغة' }).waitFor({ timeout: 5000 })
    await arabicCartQuantity.fill('2')
    await arabicCartSummary.filter({ hasText: 'لديك منتجان في سلة التسوق' }).waitFor({ timeout: 5000 })
    await page.getByRole('button', { name: 'إضافة منتج واحد' }).click()
    await arabicCartSummary.filter({ hasText: 'لديك 3 منتجات في سلة التسوق' }).waitFor({ timeout: 5000 })
  })
  await attempt('the Arabic value uses the browser Intl', () =>
    page
      .getByRole('status', { name: 'القيمة المنسقة في المتصفح' })
      .filter({ hasText: new Intl.NumberFormat('ar').format(1_250_000) })
      .waitFor({ timeout: 5000 }),
  )

  await page.getByRole('link', { name: 'English', exact: true }).click()
  await attempt('the English link loads a left-to-right document', () => page.locator('html[lang="en"][dir="ltr"]').waitFor({ timeout: 5000 }))
  await attempt('the English value resets', () =>
    page.getByRole('status', { name: 'Browser-formatted value' }).filter({ hasText: '1,250,000' }).waitFor({ timeout: 5000 }),
  )
  check('the English cart resets', (await page.getByRole('spinbutton', { name: 'Cart quantity' }).inputValue()) === '0')
  check('the English title', (await page.title()) === 'Remix i18n Demo')
  check('the switcher shows en', (await page.getByRole('combobox').inputValue()) === 'en')
  check('another new document', (await origin()) !== arabicDocumentStarted)

  await page.getByRole('combobox').selectOption('ja')
  await page.getByRole('button', { name: 'Save preference', exact: true }).click()
  await attempt('saving a preference redirects to /ja', async () => {
    await page.waitForURL((url) => url.pathname === '/ja', { timeout: 5000 })
    await page.locator('html[lang="ja"]').waitFor({ timeout: 5000 })
  })
  const saved = (await context.cookies()).find((c) => c.name === 'locale')
  check('the preference cookie is HttpOnly, Lax, one year', saved?.value === 'amE=' && saved.httpOnly && saved.sameSite === 'Lax' && saved.expires > Date.now() / 1000 + 364 * 86400, saved)

  await page.getByRole('link', { name: 'Remix i18n', exact: true }).click()
  await attempt('the cookie decides the language at /', async () => {
    await page.waitForURL((url) => url.pathname === '/', { timeout: 5000 })
    await page.locator('html[lang="ja"]').waitFor({ timeout: 5000 })
  })
  check('the switcher shows ja', (await page.getByRole('combobox').inputValue()) === 'ja')
  check('the detection card marks the cookie', (await page.locator('li[data-active="true"]').innerText()).includes('locale='))

  await page.getByRole('link', { name: 'English', exact: true }).click()
  await page.locator('html[lang="en"]').waitFor()
  await page.getByRole('combobox').selectOption('ja')
  await page.getByRole('button', { name: 'Clear saved preference', exact: true }).click()
  await attempt('clearing returns to the negotiated language', async () => {
    await page.waitForURL((url) => url.pathname === '/', { timeout: 5000 })
    await page.locator('html[lang="en"][dir="ltr"]').waitFor({ timeout: 5000 })
  })
  check('the switcher shows en again', (await page.getByRole('combobox').inputValue()) === 'en')
  check('the English title again', (await page.title()) === 'Remix i18n Demo')
  check('the preference cookie is gone', !(await context.cookies()).some((c) => c.name === 'locale'))
  if (process.env.SHOT) {
    await page.goto('/ar')
    await page.setViewportSize({ width: 1280, height: 900 })
    await page.screenshot({ path: process.env.SHOT, fullPage: true })
  }
  check('no page errors or rsmix warnings', errors.length === 0, errors)
  await context.close()
}

// --- Without JavaScript ----------------------------------------------------------------
{
  const context = await browser.newContext({ baseURL: BASE, javaScriptEnabled: false, locale: 'en-US' })
  const page = await context.newPage()
  await page.goto('/en')
  check('the value button is disabled without JS', await page.getByRole('button', { name: 'Increase value' }).isDisabled())
  check('the cart input is disabled without JS', await page.getByRole('spinbutton', { name: 'Cart quantity' }).isDisabled())
  await page.getByRole('link', { name: 'العربية', exact: true }).click()
  await attempt('links work without JS', () => page.locator('html[lang="ar"][dir="rtl"]').waitFor({ timeout: 5000 }))
  await page.getByRole('combobox').selectOption('fr')
  await page.getByRole('button', { name: 'حفظ التفضيل', exact: true }).click()
  await attempt('the preference form works without JS', () => page.waitForURL((url) => url.pathname === '/fr', { timeout: 5000 }))
  check('the French title', (await page.title()) === 'Démo i18n dans Remix', await page.title())
  await page.getByRole('link', { name: 'Remix i18n', exact: true }).click()
  await page.waitForURL((url) => url.pathname === '/')
  check('the saved French preference applies at /', (await page.getByRole('combobox').inputValue()) === 'fr')
  await page.getByRole('button', { name: 'Effacer la préférence enregistrée', exact: true }).click()
  await attempt('clearing works without JS', async () => {
    await page.waitForURL((url) => url.pathname === '/', { timeout: 5000 })
    await page.locator('html[lang="en"][dir="ltr"]').waitFor({ timeout: 5000 })
  })
  check('the switcher shows en without JS', (await page.getByRole('combobox').inputValue()) === 'en')
  await context.close()
}

await browser.close()
console.log(failures ? `${failures} failed` : 'all passed')
process.exit(failures ? 1 : 0)
