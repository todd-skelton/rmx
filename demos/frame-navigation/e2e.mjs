// Browser checks for the frame-navigation demo. Start the server first
// (`cargo run -p frame-navigation-demo`), then: node demos/frame-navigation/e2e.mjs
// Env: BASE (http://127.0.0.1:44111), PLAYWRIGHT_NODE_MODULES, CHROMIUM_PATH.
import { createRequire } from 'node:module'

const NODE_MODULES =
  process.env.PLAYWRIGHT_NODE_MODULES ??
  'C:/Users/ToddS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/'
const { chromium } = createRequire(NODE_MODULES)('playwright')
const BASE = process.env.BASE ?? 'http://127.0.0.1:44111'

const browser = await chromium.launch({
  executablePath:
    process.env.CHROMIUM_PATH ?? `${process.env.LOCALAPPDATA}/ms-playwright/chromium-1223/chrome-win64/chrome.exe`,
})
const context = await browser.newContext()
const page = await context.newPage()
const errors = []
page.on('pageerror', (e) => errors.push(e.message))
let failures = 0
const check = (name, ok, detail) => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ' ' + JSON.stringify(detail)}`)
  if (!ok) failures++
}
const ready = () => page.waitForFunction(() => document.documentElement.dataset.rmxReady === 'true', null, { timeout: 10000 })
const until = (fn, arg) => page.waitForFunction(fn, arg, { timeout: 5000 })
const soft = () => page.evaluate(() => window.__keep === true)
const mark = () => page.evaluate(() => (window.__keep = true))

// Auth: protected routes send you to login; logging in sets a cookie.
await page.goto(BASE + '/')
await ready()
check('auth: signed-out visitors land on login', (await page.evaluate(() => location.pathname)) === '/auth/login')
await page.click('#login')
await until(() => document.querySelector('#page-title')?.innerText === 'Dashboard')
check('auth: login sets the cookie and redirects home', (await context.cookies()).some((c) => c.name === 'frame-navigation-auth'))

// link() mixin on a non-anchor element: keyboard, and Ctrl-click opens a tab.
await mark()
await page.focus('.stat-card >> nth=1')
await page.keyboard.press('Enter')
await until(() => document.querySelector('#page-title')?.innerText === 'Calendar')
check('link mixin: Enter on a card navigates softly', await soft())
await page.goBack()
await until(() => document.querySelector('#page-title')?.innerText === 'Dashboard')
const [popup] = await Promise.all([context.waitForEvent('page'), page.click('.stat-card >> nth=0', { modifiers: ['Control'] })])
check('link mixin: Ctrl-click opens a new tab', new URL(popup.url()).pathname === '/courses' && (await soft()))
await popup.close()

// Courses: a GET form targeting a named frame, replacing history, keeping focus.
await page.click('#main-nav a[href="/courses"]')
await page.waitForSelector('#course-filter')
await page.evaluate(() => (window.__input = document.querySelector('#course-filter')))
const entries = await page.evaluate(() => navigation.entries().length)
await page.focus('#course-filter')
await page.keyboard.type('web', { delay: 80 })
await until(() => document.querySelector('#course-results')?.innerText.includes('“web”'))
await page.waitForTimeout(200)
const courses = await page.evaluate(() => ({
  same: window.__input === document.querySelector('#course-filter'),
  focus: document.activeElement?.id,
  url: location.search,
  items: document.querySelectorAll('#course-list li').length,
  entries: navigation.entries().length,
}))
check('courses: frame filter keeps the input, focus and history', courses.same && courses.focus === 'course-filter' && courses.url === '?q=web' && courses.items === 2 && courses.entries === entries, courses)
await page.click('#clear-filter')
await until(() => document.querySelector('#course-results')?.innerText === '4 courses')
check('courses: clear link targets the frame', (await page.evaluate(() => location.search)) === '' && (await soft()))

// Account: edit in a named frame, 422 validation, redirect after save.
await page.click('#main-nav a[href="/account"]')
await page.click('#edit-account')
await page.waitForSelector('#account-form')
check('account: edit link loads into the account frame', (await page.evaluate(() => location.pathname)) === '/account/edit' && (await soft()))
await page.evaluate(() => (document.querySelector('#account-form').noValidate = true))
await page.fill('#displayName', '')
await page.click('#account-form button[type=submit]')
await page.waitForSelector('.field-error')
check('account: 422 shows validation errors in the frame', (await page.locator('.field-error').innerText()) === 'Enter a display name.' && (await soft()))
await page.evaluate(() => (document.querySelector('#account-form').noValidate = true))
await page.fill('#displayName', 'Rmx Rustacean')
await page.click('#account-form button[type=submit]')
await page.waitForSelector('#saved')
const saved = await page.evaluate(() => [location.pathname + location.search, document.querySelector('#account-details').innerText.includes('Rmx Rustacean')])
check('account: save redirects to the saved view', saved[0] === '/account?saved' && saved[1] && (await soft()), saved)

// Settings: nested navigation inside a named frame, back button.
await page.click('#main-nav a[href="/settings"]')
await page.waitForSelector('#settings-nav')
await page.click('#settings-nav a[href="/settings/notifications"]')
await until(() => document.querySelector('#settings-title')?.innerText === 'Notifications')
check('settings: frame navigation updates the URL', (await page.evaluate(() => location.pathname)) === '/settings/notifications' && (await soft()))
await page.goBack()
await until(() => document.querySelector('#settings-title')?.innerText === 'Overview')
check('settings: back restores the previous frame content', await soft())

// A protected frame navigation that redirects to login loads the login document.
await context.clearCookies()
await page.click('#settings-nav a[href="/settings/grading"]')
await page.waitForSelector('#login-title')
check('auth: redirected frame navigation falls back to the login document', (await page.evaluate(() => location.pathname)) === '/auth/login' && !(await page.evaluate(() => !!document.querySelector('#settings-content'))))

// Errors: a 500 in a frame renders the resolver's reload card in place.
await page.click('#login')
await until(() => document.querySelector('#page-title')?.innerText === 'Dashboard')
await page.goto(BASE + '/settings/overview')
await ready()
await mark()
await page.click('#settings-nav a[href="/settings/privacy"]')
await page.waitForSelector('.error-card', { timeout: 5000 })
check(
  'errors: a 500 frame response shows the reload card in the frame',
  (await page.locator('.error-card h1').innerText()) === 'Reload required' &&
    (await page.evaluate(() => location.pathname)) === '/settings/privacy' &&
    (await soft()) &&
    !!(await page.$('#main-nav')),
)
check('errors: the reload link opts out of soft navigation', (await page.getAttribute('.error-card a', 'data-rmx-document')) === '')

// A failed frame request reaches the app error listener, which replaces the page.
await page.goto(BASE + '/settings/overview')
await ready()
await page.route('**/settings/grading', (route) => route.abort())
await page.click('#settings-nav a[href="/settings/grading"]')
await page.waitForSelector('#app-error .error-card', { timeout: 5000 })
check(
  'errors: a failed load shows the app error page',
  (await page.locator('#app-error h1').innerText()) === 'Something went wrong' && /fetch/i.test(await page.locator('#app-error .error-card p >> nth=1').innerText()),
  await page.locator('#app-error').innerText(),
)
await page.unroute('**/settings/grading')
// The runtime is disposed: links are plain document navigations now.
check('errors: the disposed runtime no longer intercepts navigation', !(await page.$('#main-nav')))

check('no page errors', errors.length === 0, errors)
await browser.close()
console.log(failures ? `${failures} failed` : 'all passed')
process.exit(failures ? 1 : 0)
