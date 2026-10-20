// Browser checks for the social-auth demo. Start the server first
// (`cargo run -p social-auth-demo`), then: node examples/social-auth/e2e.mjs
// Env: BASE (http://127.0.0.1:44106), PLAYWRIGHT_NODE_MODULES, CHROMIUM_PATH.
import { createRequire } from 'node:module'

const NODE_MODULES =
  process.env.PLAYWRIGHT_NODE_MODULES ??
  'C:/Users/ToddS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/'
const { chromium } = createRequire(NODE_MODULES)('playwright')
const BASE = process.env.BASE ?? 'http://127.0.0.1:44106'

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
const path = () => new URL(page.url()).pathname
const text = (sel) => page.locator(sel).first().innerText()
const signIn = async (email, password) => {
  await page.fill('#email', email)
  await page.fill('#password', password)
  await Promise.all([page.waitForNavigation(), page.click('button[type=submit]')])
}

await page.goto(BASE + '/')
check('home: login page', (await page.title()) === 'Social Auth Demo' && (await text('h1')) === 'Welcome Back')
const providers = await page.$$eval('button[data-provider]', (bs) => bs.map((b) => [b.dataset.provider, b.disabled, b.title]))
check(
  'home: provider buttons disabled with the missing env vars',
  providers.length === 3 && providers.every(([, d, t]) => d && /_CLIENT_ID and .*_CLIENT_SECRET/.test(t)),
  providers,
)
const favicon = await page.request.get(BASE + '/favicon.svg')
check('static: favicon served', favicon.status() === 200 && favicon.headers()['content-type'].includes('svg'))

await signIn('user@example.com', 'wrong-password')
check('login: bad password flashes an error', path() === '/' && (await text('.notice-error')).includes('Invalid email or password'))
await page.reload()
check('login: flash shows once', (await page.locator('.notice-error').count()) === 0)

await signIn('user@example.com', 'password123')
check('login: lands on account', path() === '/account' && (await page.locator('text=Demo User').count()) > 0)
const cookie = (await context.cookies()).find((c) => c.name === 'social-auth-session')
check('session cookie: HttpOnly, Lax, 30 days', cookie?.httpOnly && cookie.sameSite === 'Lax' && cookie.expires - Date.now() / 1000 > 29 * 86400, cookie)
check('account: JSON dump of the identity', (await text('pre')).includes('"loginMethod": "credentials"'))

await Promise.all([page.waitForNavigation(), page.click('text=Logout')])
check('logout: back to login', path() === '/')
await page.goto(BASE + '/account')
check('logout: account now redirects home', path() === '/')

await page.click('text=Sign up')
await page.fill('#name', 'Grace Hopper')
await page.fill('#email', 'user@example.com')
await page.fill('#password', 'password123')
await Promise.all([page.waitForNavigation(), page.click('button[type=submit]')])
check('signup: duplicate email rejected, values kept', (await text('.notice-error')).includes('already exists') && (await page.inputValue('#name')) === 'Grace Hopper')
await page.fill('#email', 'grace@example.com')
await page.fill('#password', 'password123')
await Promise.all([page.waitForNavigation(), page.click('button[type=submit]')])
check('signup: signed in as the new user', path() === '/account' && (await page.locator('text=Grace Hopper').count()) > 0)
await Promise.all([page.waitForNavigation(), page.click('text=Logout')])

await page.click('text=Forgot password?')
await page.fill('#email', 'admin@example.com')
await Promise.all([page.waitForNavigation(), page.click('button[type=submit]')])
check('forgot: reset link shown', (await text('h1')) === 'Check Your Email' && (await page.locator('a.reset-link').count()) === 1)
await Promise.all([page.waitForNavigation(), page.click('text=Open Reset Form')])
await page.fill('#password', 'brand-new-pass')
await page.fill('#confirmPassword', 'brand-new-pass')
await Promise.all([page.waitForNavigation(), page.click('button[type=submit]')])
check('reset: password updated', (await text('h1')) === 'Password Updated')
await Promise.all([page.waitForNavigation(), page.click('text=Back to Sign In')])
check('reset: success flash on the login page', (await text('.notice-success')).includes('Password updated'))
await signIn('admin@example.com', 'brand-new-pass')
check('reset: new password works', path() === '/account' && (await page.locator('text=Demo Admin').count()) > 0)

if (process.env.SHOT) await page.screenshot({ path: process.env.SHOT, fullPage: true })
check('no page errors', errors.length === 0, errors)
await browser.close()
console.log(failures ? `${failures} FAILED` : 'all passed')
process.exit(failures ? 1 : 0)
