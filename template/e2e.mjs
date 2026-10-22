// Browser checks for the starter app. Start the server first (`cargo run -p template-demo`),
// then: node template/e2e.mjs
// Env: BASE (http://127.0.0.1:44113), TITLE (rmx App; an app made by `rmx new` uses its
// name), PLAYWRIGHT_NODE_MODULES, CHROMIUM_PATH.
import { createRequire } from 'node:module'

const NODE_MODULES =
  process.env.PLAYWRIGHT_NODE_MODULES ??
  'C:/Users/ToddS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/'
const { chromium } = createRequire(NODE_MODULES)('playwright')
const BASE = process.env.BASE ?? 'http://127.0.0.1:44113'
const TITLE = process.env.TITLE ?? 'rmx App'
const FIRST_PROMPT = 'I want to build a simple headless Shopify store, what does Remix have available to help scaffold this?'

const browser = await chromium.launch({
  executablePath:
    process.env.CHROMIUM_PATH ?? `${process.env.LOCALAPPDATA}/ms-playwright/chromium-1223/chrome-win64/chrome.exe`,
})
const context = await browser.newContext({ permissions: ['clipboard-read', 'clipboard-write'] })
const page = await context.newPage()
const errors = []
page.on('pageerror', (e) => errors.push(e.message))
page.on('console', (m) => m.type() === 'warning' && m.text().startsWith('rmx') && errors.push(m.text()))
let failures = 0
const check = (name, ok, detail) => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ' ' + JSON.stringify(detail)}`)
  if (!ok) failures++
}
const ready = () => page.waitForFunction(() => document.documentElement.dataset.rmxReady === 'true', null, { timeout: 10000 })
const until = (fn, arg, timeout = 5000) => page.waitForFunction(fn, arg, { timeout })
// The status label (first inner span of the label slot) and whether it shows.
const firstButton = () =>
  page.evaluate(() => {
    let button = document.querySelector('main button')
    let [status, prompt] = button.querySelectorAll(':scope > span:last-child > span')
    return {
      status: status.textContent,
      statusVisible: getComputedStyle(status).visibility === 'visible',
      promptVisible: getComputedStyle(prompt).visibility === 'visible',
      background: button.style.background,
    }
  })

const html = await (await fetch(BASE + '/')).text()
check('server renders the document with the client entry script', html.startsWith('<!DOCTYPE html>') && html.includes(`<title>${TITLE}</title>`) && html.includes('/assets/app/actions/public/entry.js'))
check('no live reload script outside rmx dev', !html.includes('/__rmx/live-reload'))
check('server renders four prompt buttons', (html.match(/<button type="button"/g) ?? []).length === 4)
check('favicon is served from public/', (await fetch(BASE + '/favicon.svg')).headers.get('content-type') === 'image/svg+xml')

await page.goto(BASE + '/')
await ready()
let before = await firstButton()
check('prompt buttons start idle', before.promptVisible && !before.statusVisible && before.status.includes(FIRST_PROMPT), before)

await page.click('main button')
await until(() => document.querySelector('main button').textContent.includes('Copied to clipboard'))
let copied = await firstButton()
check('clicking confirms the copy', copied.statusVisible && !copied.promptVisible && copied.status === 'Copied to clipboard', copied)
check('the active button is highlighted', copied.background !== '', copied)
check('the prompt is on the clipboard', (await page.evaluate(() => navigator.clipboard.readText())) === FIRST_PROMPT)
await until(() => !document.querySelector('main button').textContent.includes('Copied to clipboard'), null, 4000)
let after = await firstButton()
check('the button resets after holding and fading', after.promptVisible && !after.statusVisible && after.background === '', after)

await page.emulateMedia({ colorScheme: 'dark' })
let dark = await page.evaluate(() => getComputedStyle(document.querySelector('main')).backgroundColor)
check('dark mode swaps the design tokens', dark === 'rgb(30, 34, 38)', dark)

check('no page errors or rmx warnings', errors.length === 0, errors)
await browser.close()
console.log(failures ? `${failures} failed` : 'all passed')
process.exit(failures ? 1 : 0)
