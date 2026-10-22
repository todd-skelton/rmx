// Browser checks for the assets demo. Build the client and start the server first:
//   ./scripts/build-client.ps1 -Crate assets-demo -Out demos/assets/app/actions/public/pkg
//   cargo run -p assets-demo
// then: node demos/assets/e2e.mjs
// Env: BASE (http://127.0.0.1:44104), PLAYWRIGHT_NODE_MODULES, CHROMIUM_PATH.
import { readFileSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'

const NODE_MODULES =
  process.env.PLAYWRIGHT_NODE_MODULES ??
  'C:/Users/ToddS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/'
const { chromium } = createRequire(NODE_MODULES)('playwright')
const BASE = process.env.BASE ?? 'http://127.0.0.1:44104'
const PUBLIC = new URL('./app/actions/public/', import.meta.url)

const browser = await chromium.launch({
  executablePath:
    process.env.CHROMIUM_PATH ?? `${process.env.LOCALAPPDATA}/ms-playwright/chromium-1223/chrome-win64/chrome.exe`,
})
const page = await browser.newPage()
const errors = []
const failed = []
page.on('pageerror', (e) => errors.push(e.message))
page.on('response', (r) => r.status() >= 400 && failed.push(`${r.status()} ${r.url()}`))
let failures = 0
const check = (name, ok, detail) => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ' ' + JSON.stringify(detail)}`)
  if (!ok) failures++
}
const mounted = () => page.waitForSelector('#app-root .client-card', { timeout: 10000 })
const card = () => page.evaluate(() => document.querySelector('#app-root')?.innerText)

const html = await (await fetch(BASE + '/')).text()
check('server HTML shows the loading state', html.includes('Loading client app...'))
check('server HTML preloads the module graph', (html.match(/rel="modulepreload"/g) ?? []).length === 3, html)

await page.goto(BASE + '/')
await mounted()
check('the wasm client card replaces the loading text', (await card())?.includes('Client code is live'), await card())
check('the card shows text from content.js', (await card())?.includes('Edit app/actions/public/content.js'), await card())

const images = await page.evaluate(() => [...document.querySelectorAll('img.asset-image')].map((i) => [i.complete, i.naturalWidth]))
check('both <img> assets load', images.length === 2 && images.every(([done, w]) => done && w > 0), images)

const backgrounds = await page.evaluate(() =>
  [...document.querySelectorAll('.asset-background')].map((d) => getComputedStyle(d).backgroundImage),
)
check(
  'CSS url() references resolve to asset URLs',
  backgrounds[0]?.includes('/assets/app/actions/public/images/image.svg"'),
  backgrounds,
)
check(
  'the CSS request transform is kept in the rewritten URL',
  backgrounds[1]?.includes('image.svg?transform=recolor%3Aff0000'),
  backgrounds,
)
const red = await (await fetch(BASE + '/assets/app/actions/public/images/image.svg?transform=recolor%3Aff0000')).text()
check('the CSS background SVG is recolored', red.includes('fill="#ff0000"'), red)
const purpleSrc = await page.evaluate(() => document.querySelectorAll('img.asset-image')[1].src)
const purple = await (await fetch(purpleSrc)).text()
check('the transformed <img> is purple', purple.includes('#8B5CF6') && !purple.includes('currentColor'), purple)

// Edits show up after a refresh, without restarting the server.
const contentFile = new URL('content.js', PUBLIC)
const cssFile = new URL('styles/app.css', PUBLIC)
const content = readFileSync(contentFile, 'utf8')
const css = readFileSync(cssFile, 'utf8')
try {
  writeFileSync(contentFile, content.replace(/return '[^']*'/, "return 'Edited while the server was running.'"))
  writeFileSync(cssFile, css + '\nh1 { color: rgb(220, 38, 38); }\n')
  await page.reload()
  await mounted()
  check('an edited module is served on refresh', (await card())?.includes('Edited while the server was running.'), await card())
  const h1 = await page.evaluate(() => getComputedStyle(document.querySelector('h1')).color)
  check('an edited stylesheet is served on refresh', h1 === 'rgb(220, 38, 38)', h1)
} finally {
  writeFileSync(contentFile, content)
  writeFileSync(cssFile, css)
}
await page.reload()
await mounted()
check('restored files are served again', (await card())?.includes('Edit app/actions/public/content.js'), await card())

check('no page errors', errors.length === 0, errors)
check('no failed requests', failed.length === 0, failed)
await browser.close()
console.log(failures ? `${failures} failed` : 'all passed')
process.exit(failures ? 1 : 0)
