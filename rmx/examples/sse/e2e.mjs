// Browser checks for the sse demo. Start the server first (`cargo run -p sse-demo`),
// then: node examples/sse/e2e.mjs
// Env: BASE (http://127.0.0.1:44102), PLAYWRIGHT_NODE_MODULES, CHROMIUM_PATH.
import { createRequire } from 'node:module'

const NODE_MODULES =
  process.env.PLAYWRIGHT_NODE_MODULES ??
  'C:/Users/ToddS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/'
const { chromium } = createRequire(NODE_MODULES)('playwright')
const BASE = process.env.BASE ?? 'http://127.0.0.1:44102'

const browser = await chromium.launch({
  executablePath:
    process.env.CHROMIUM_PATH ?? `${process.env.LOCALAPPDATA}/ms-playwright/chromium-1223/chrome-win64/chrome.exe`,
})
const context = await browser.newContext()
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
const status = () => page.evaluate(() => document.getElementById('connection')?.innerText.trim())
const messages = () => page.evaluate(() => [...document.querySelectorAll('li.message')].map((li) => li.innerText))

// Server HTML: disconnected, waiting.
const html = await (await fetch(BASE + '/?limit=3')).text()
check('server renders the disconnected state', html.includes('Disconnected') && html.includes('Waiting for messages...'))

const streamResponses = []
page.on('response', (r) => r.url().includes('/messages') && streamResponses.push(r))
await page.goto(BASE + '/?limit=3')
await ready()
await until(() => document.getElementById('connection')?.innerText.includes('Connected'))
check('the client entry opens the stream after hydration', (await status()) === 'Connected')
await until(() => document.querySelectorAll('li.message').length >= 1, null, 3000)
check('messages arrive about once a second', (await messages())[0]?.startsWith('#1 Message #1 at'), await messages())
await until(() => document.getElementById('connection')?.innerText.includes('Disconnected'), null, 6000)
const got = await messages()
check('the stream closes after the limit', got.length === 3 && got[2].startsWith('#3'), got)
await page.waitForTimeout(1500)
check('a closed stream does not reconnect', (await messages()).length === 3)
const headers = await streamResponses[0]?.allHeaders()
check('the stream is text/event-stream', headers?.['content-type'] === 'text/event-stream', headers)
check('the stream is compressed', headers?.['content-encoding'] === 'gzip', headers)

// No limit: the link adds one; leaving the page closes the stream.
await page.goto(BASE + '/')
await ready()
check('no limit by default', (await page.evaluate(() => document.getElementById('limit-info').innerText)).startsWith('No limit set.'))
await until(() => document.querySelectorAll('li.message').length >= 2, null, 4000)
check('an unlimited stream keeps going', (await status()) === 'Connected')
await page.click('#limit-info a')
await until(() => document.getElementById('limit-info').innerText.includes('Stream will close after 10 messages.'))
check('the limit link navigates', new URL(page.url()).search === '?limit=10')

check('no page errors or rmx warnings', errors.length === 0, errors)
await browser.close()
console.log(failures ? `${failures} failed` : 'all passed')
process.exit(failures ? 1 : 0)
