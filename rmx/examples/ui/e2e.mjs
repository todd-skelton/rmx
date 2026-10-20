// Browser checks for the ui demo gallery. Build the client and start the server first:
//   ./build-client.ps1 -Crate ui-demo -Out examples/ui/public/pkg
//   cargo run -p ui-demo
// then: node examples/ui/e2e.mjs
// Env: BASE (http://127.0.0.1:44105), PLAYWRIGHT_NODE_MODULES, CHROMIUM_PATH.
import { createRequire } from 'node:module'

const NODE_MODULES =
  process.env.PLAYWRIGHT_NODE_MODULES ??
  'C:/Users/ToddS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/'
const { chromium } = createRequire(NODE_MODULES)('playwright')
const BASE = process.env.BASE ?? 'http://127.0.0.1:44105'

const browser = await chromium.launch({
  executablePath:
    process.env.CHROMIUM_PATH ?? `${process.env.LOCALAPPDATA}/ms-playwright/chromium-1223/chrome-win64/chrome.exe`,
})
const page = await browser.newPage()
const errors = []
page.on('pageerror', (e) => errors.push(e.message))
page.on('console', (m) => m.type() === 'error' && !m.text().startsWith('Failed to load resource') && errors.push(m.text()))
// Failed requests by URL; the browser's automatic /favicon.ico request is not the demo's.
page.on('response', (r) => r.status() >= 400 && !r.url().endsWith('/favicon.ico') && errors.push(`${r.status()} ${r.url()}`))
let failures = 0
const check = (name, ok, detail) => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ' ' + JSON.stringify(detail)}`)
  if (!ok) failures++
}
const open = async (slug) => {
  await page.goto(`${BASE}/demo/${slug}`)
  await page.waitForFunction(() => document.documentElement.dataset.rmxReady === 'true', null, { timeout: 10000 })
}
const settle = () => page.waitForTimeout(50)
const eq = (a, b) => JSON.stringify(a) === JSON.stringify(b)

// Index
await page.goto(BASE + '/')
const links = await page.$$eval('a[href^="/demo/"]', (as) => as.map((a) => a.getAttribute('href')))
check('the index lists the demos', links.length === 6, links)

// Basic counter
await open('component/runtime/basic')
await page.click('button')
await page.click('button')
await settle()
check('basic counter counts clicks', (await page.textContent('button')) === "Ye ol' counter: 2", await page.textContent('button'))

// Keyed list
await open('component/runtime/keyed-list')
const rows = () => page.$$eval('.list-item input', (els) => els.map((e) => e.placeholder + '=' + e.value))
check('keyed inputs start from defaultValue', eq(await rows(), ['Item A=Item A', 'Item B=Item B', 'Item C=Item C', 'Item D=Item D']), await rows())
await page.fill('.list-item:nth-child(3) input', 'edited B')
await page.click('text=Reverse List')
await settle()
check('reverse moves rows with their DOM state', eq(await rows(), ['Item D=Item D', 'Item C=Item C', 'Item B=edited B', 'Item A=Item A']), await rows())
await page.click('.list-item:nth-child(3) button')
await settle()
check('move up swaps neighbors', eq(await rows(), ['Item C=Item C', 'Item D=Item D', 'Item B=edited B', 'Item A=Item A']), await rows())
await page.click('text=Start Auto-Shuffle')
await settle()
check('auto-shuffle toggles its label', (await page.textContent('.controls button:nth-child(3)')) === 'Stop Auto-Shuffle')
let orders = new Set()
for (let i = 0; i < 4; i++) {
  orders.add((await rows()).join())
  await page.waitForTimeout(1050)
}
await page.click('text=Stop Auto-Shuffle')
check('auto-shuffle reorders every second', orders.size > 1, [...orders])
check('shuffled rows keep their edited value', (await rows()).includes('Item B=edited B'), await rows())

// Controlled and uncontrolled values
await open('component/runtime/controlled-uncontrolled-values')
const val = (s) => page.$eval(s, (el) => (el.type === 'checkbox' ? el.checked : el.value))
await page.click('#controlled-text')
await page.keyboard.press('End')
await page.keyboard.type('x1y')
await settle()
check('a controlled input rejects input the app does not accept', (await val('#controlled-text')) === 'helloxy', await val('#controlled-text'))
await page.click('#controlled-checked')
await page.selectOption('#controlled-choice', 'gamma')
await settle()
check(
  'controlled checkbox and select follow state',
  (await page.textContent('#controlled-snapshot')) === 'State snapshot: text="helloxy", checked=false, choice="gamma"',
  await page.textContent('#controlled-snapshot'),
)
await page.click('#reset')
await settle()
check('reset puts controlled values back', eq([await val('#controlled-text'), await val('#controlled-checked'), await val('#controlled-choice')], ['hello', true, 'alpha']))
await page.fill('#uncontrolled-text', 'abc')
await page.click('#uncontrolled-checked')
await page.click('#force')
await settle()
check('uncontrolled values survive re-renders', eq([await val('#uncontrolled-text'), await val('#uncontrolled-checked')], ['abc', false]))
await page.click('#remount')
await settle()
check('a new key remounts uncontrolled inputs with their defaults', eq([await val('#uncontrolled-text'), await val('#uncontrolled-checked')], ['type to update this', true]))

// README examples
await open('component/runtime/readme')
await page.waitForTimeout(100)
check('every README example renders', (await page.$$eval('.example h2', (hs) => hs.length)) === 19)
await page.click('#search')
await page.keyboard.type('ab', { delay: 50 })
check('search shows loading at once', await page.evaluate(() => document.body.innerText.includes('Loading...')))
await page.waitForTimeout(450)
const results = await page.$$eval('#search ~ ul li', (lis) => lis.map((l) => l.textContent))
check('the event signal drops the stale search', eq(results, ['Result for "ab" 1', 'Result for "ab" 2']), results)
await page.fill('#slug', 'my-slug')
await page.click('#auto-slug')
await settle()
const generated = await page.$eval('#slug', (e) => [e.value, e.disabled])
check('a generated slug replaces the controlled value', /^[0-9a-f]{8}$/.test(generated[0]) && generated[1], generated)
await page.click('#auto-slug')
await settle()
check('unchecking restores the typed slug', eq(await page.$eval('#slug', (e) => [e.value, e.disabled]), ['my-slug', false]))
await page.keyboard.press('x')
await settle()
check('a document keydown listener from queue_task tracks keys', (await page.textContent('#keys')) === 'Keys: a, b, x', await page.textContent('#keys'))
await page.click('#select-input')
check('a ref selects the input', await page.evaluate(() => document.activeElement?.id === 'select-me'))
check('a ref with a signal runs a ResizeObserver', /Resize me! \(\d+ \u00d7 60\)/.test(await page.textContent('#resize')), await page.textContent('#resize'))
await page.click('#play')
await page.waitForTimeout(80)
check('updated() resolves after the DOM update', eq([await page.$eval('#play', (e) => e.disabled), await page.evaluate(() => document.activeElement?.id)], [true, 'stop']))
await page.click('#show-details')
await page.waitForTimeout(500)
check('queue_task scrolls to the new section', (await page.$eval('#details', (e) => Math.round(e.getBoundingClientRect().top))) < 100)
const t1 = await page.textContent('#clock')
await page.waitForTimeout(1100)
check('the clock ticks', t1 !== (await page.textContent('#clock')))
check('dom_id pairs label and input', await page.evaluate(() => { const l = [...document.querySelectorAll('label')].find((l) => l.textContent === 'Name'); return !!document.getElementById(l.htmlFor) }))
check('context reaches the themed header', (await page.$eval('#themed-header', (e) => getComputedStyle(e).backgroundColor)) === 'rgb(0, 0, 0)')
await page.click('#toggle-theme')
await settle()
check('a TypedEventTarget in context updates the consumer', (await page.textContent('#themed-content')) === 'Current theme: dark')
check('fragments flatten into the list', (await page.$$eval('#fragment-list li', (l) => l.length)) === 3)

// Mixin lifecycle
await open('component/runtime/mixin-lifecycle')
await settle()
const log = async () => ((await page.evaluate(() => document.documentElement.dataset.mixinLog)) ?? '').split('|')
check('insert fires once per slot after hydration', eq(await log(), ['insert:button:0:button', 'insert:box:div']), await log())
check('the server rendered the mixin attributes', (await page.getAttribute('#save', 'aria-label')) === 'Save: 0 clicks')
await page.click('#save')
await page.click('#save')
await settle()
check('mixin update() re-runs the mixin', (await page.getAttribute('#save', 'aria-label')) === 'Save: 2 clicks')
check('mixin update() does not render the owner', (await page.textContent('#owner-renders')) === '1')
const id = await page.getAttribute('#traced', 'data-mixin-id')
await page.click('#rerender')
await settle()
check('owner updates send beforeUpdate and commit', eq((await log()).slice(2), ['beforeUpdate:box', 'commit:box']), await log())
check('slots keep their state across owner renders', (await page.getAttribute('#save', 'aria-label')) === 'Save: 2 clicks' && (await page.getAttribute('#traced', 'data-mixin-id')) === id)
await page.click('#toggle-tracer')
await settle()
check('dropping a mixin from mix disposes it', (await log()).at(-1) === 'remove:box' && (await page.getAttribute('#traced', 'data-mixin-id')) === null, await log())
await page.click('#toggle-tracer')
await settle()
check('adding a mixin to a mounted element inserts it', (await log()).at(-1) === 'insert:box:div', await log())
await page.click('#rows li:nth-child(2) .remove')
await settle()
const fades = () => page.$$eval('#rows li', (lis) => lis.map((li) => li.dataset.fade + (li.hasAttribute('data-leaving') ? '*' : '')))
check('persist_node keeps a removed row in the document', eq(await fades(), ['row1', 'row2*', 'row3']), await fades())
await page.waitForTimeout(400)
check('the row leaves when its teardown finishes', eq(await fades(), ['row1', 'row3']), await fades())
check('beforeRemove, teardown, then remove', eq((await log()).filter((l) => l.endsWith('row2') || l.includes(':row2:')), ['beforeRemove:row2', 'persisted:row2:false', 'remove:row2']), await log())
await page.click('#add-row')
await page.click('#clear-rows')
await settle()
check('clearing a list persists every row', eq(await fades(), ['row1*', 'row3*', 'row4*']), await fades())
await page.waitForTimeout(400)
check('cleared rows leave after their teardown', eq(await fades(), []), await fades())

// Draggable mixin
await open('cases/draggable')
await page.evaluate(() => {
  window.__drag = []
  for (const t of ['rmx:dragstart', 'rmx:dragend']) document.addEventListener(t, (e) => window.__drag.push(`${t}:${e.detail.left},${e.detail.top}`))
})
const bb = await (await page.$('#draggable-box')).boundingBox()
await page.mouse.move(bb.x + 20, bb.y + 20)
await page.mouse.down()
await page.mouse.move(bb.x + 80, bb.y + 60, { steps: 5 })
check('dragging sets the grabbing cursor', (await page.$eval('#draggable-box', (e) => e.style.cursor)) === 'grabbing')
await page.mouse.move(bb.x + 120, bb.y + 100, { steps: 5 })
await page.mouse.up()
await settle()
const pos = await page.$eval('#draggable-box', (e) => [e.style.left, e.style.top, e.style.cursor])
check('the box follows the pointer', eq(pos, ['124px', '104px', 'grab']), pos)
check('drag start and end events carry the position', eq(await page.evaluate(() => window.__drag), ['rmx:dragstart:24,24', 'rmx:dragend:124,104']), await page.evaluate(() => window.__drag))

check('no page errors or failed requests', errors.length === 0, errors)
await browser.close()
console.log(failures ? `${failures} failed` : 'all passed')
process.exit(failures ? 1 : 0)
