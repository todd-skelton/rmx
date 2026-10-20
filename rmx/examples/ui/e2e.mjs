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
check('the index lists the demos', links.length === 12, links)

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

// Animation gallery (rmx-ui)
await open('ui/animation')
await page.waitForTimeout(50)
check('entrance animations run on insert', await page.evaluate(() => document.querySelector('#enter .ball').getAnimations().length === 1))
check('animate_entrance(false) skips the first render', await page.evaluate(() => document.querySelector('#exit .card').getAnimations().length === 0))
await page.click('#exit .toggle')
await page.waitForTimeout(30)
check('a removed element stays while its exit animation runs', await page.evaluate(() => document.querySelector('#exit .card')?.getAnimations()[0]?.playState === 'running'))
await page.waitForTimeout(600)
check('the element leaves after its exit animation', await page.evaluate(() => !document.querySelector('#exit .card')))
await page.click('#exit .toggle')
await page.waitForTimeout(30)
check('showing again animates the entrance', await page.evaluate(() => document.querySelector('#exit .card')?.getAnimations()[0]?.effect.getKeyframes()[0].transform === 'scale(0)'))
await page.evaluate(() => { document.querySelector('#reclaim .card').__mark = 'original' })
await page.click('#reclaim .hide')
await page.waitForTimeout(80)
await page.click('#reclaim .show')
await page.waitForTimeout(30)
const reclaimed = await page.evaluate(() => [...document.querySelectorAll('#reclaim .card')].map((c) => [c.__mark, c.getAnimations().length, Number(c.getAnimations()[0]?.effect.getKeyframes()[0].opacity)]))
check('a keyed element rendered mid-exit is reclaimed and retargets', reclaimed.length === 1 && reclaimed[0][0] === 'original' && reclaimed[0][1] === 1 && reclaimed[0][2] > 0 && reclaimed[0][2] < 1, reclaimed)
await page.waitForTimeout(500)
check('the reclaimed element settles at its natural styles', await page.evaluate(() => { const c = document.querySelector('#reclaim .card'); return getComputedStyle(c).opacity === '1' && !c.getAttribute('style') }))
await page.click('#presence .remove')
await page.waitForTimeout(40)
check('a removed list row exits in place', (await page.$$eval('#presence .row', (r) => r.length)) === 3)
await page.waitForTimeout(400)
await page.click('#presence .add')
await page.waitForTimeout(40)
check('rows enter and leave by key', eq(await page.$$eval('#presence .row span', (r) => r.map((x) => x.textContent)), ['Row 4', 'Row 1', 'Row 2']))
await page.click('#flip .switch')
await page.waitForTimeout(30)
check('animate_layout plays the inverse transform', await page.evaluate(() => document.querySelector('#flip .knob').getAnimations()[0]?.effect.getKeyframes()[0].transform === 'translate3d(40px, 0px, 0px)'))
await page.waitForTimeout(400)
check('the layout animation clears its projection styles', await page.evaluate(() => { const k = document.querySelector('#flip .knob'); return k.getAnimations().length === 0 && !k.getAttribute('style') }))
await page.click('#default-animate .add')
await page.click('#default-animate .add')
await page.waitForTimeout(300)
const order = () => page.$$eval('#default-animate .item span', (r) => r.map((x) => x.textContent).join())
const start = await order()
for (let i = 0; i < 10 && (await order()) === start; i++) {
  await page.click('#default-animate .shuffle')
  await page.waitForTimeout(20)
}
check('moved list items animate from their old position', await page.evaluate(() => [...document.querySelectorAll('#default-animate .item')].some((i) => /^translate3d/.test(i.getAnimations()[0]?.effect.getKeyframes()[0].transform ?? ''))))
await page.waitForTimeout(3300)
check('a spring iterated per frame counts to 100', (await page.textContent('#html-content .count')) === '100', await page.textContent('#html-content .count'))
await page.click('#enter .replay')
await page.waitForTimeout(30)
check('replay remounts the tile and replays its entrance', await page.evaluate(() => document.querySelector('#enter .ball').getAnimations().length === 1))

// Toggle primitives (packages/ui/src/demos/toggle.demo.tsx, toggle.test.tsx)
{
await open('ui/toggle')
await page.evaluate(() => {
  window.__toggles = []
  document.addEventListener('rmx:toggle-change', (e) => window.__toggles.push([e.target.textContent.trim() || e.target.name, e.checked]))
})
const sw = () => page.$$eval('[role="switch"]', (els) => els.map((e) => [e.tagName, e.dataset.state, e.getAttribute('aria-checked'), e.checked ?? null, e.getAttribute('tabindex')]))
const s0 = await sw()
check('toggle: SSR state for native, custom and disabled switches', eq(s0, [['INPUT', 'checked', null, true, null], ['BUTTON', 'unchecked', 'false', null, '0'], ['BUTTON', 'unchecked', 'false', null, null]]), s0)
await page.click('input[role="switch"]')
await settle()
await page.click('button[role="switch"]:not([disabled])')
await settle()
await page.focus('button[role="switch"]:not([disabled])')
await page.keyboard.press(' ')
await page.keyboard.press(' ')
await settle()
await page.click('button[role="switch"][disabled]', { force: true })
await settle()
const s1 = await sw()
const toggles = await page.evaluate(() => window.__toggles)
check('toggle: click and Space flip state, disabled stays put', eq(s1.map((s) => s[1]), ['unchecked', 'checked', 'unchecked']) && s1[0][3] === false, s1)
check('toggle: rmx:toggle-change bubbles with the new state', eq(toggles, [['notifications', false], ['Desktop alerts', true], ['Desktop alerts', false], ['Desktop alerts', true]]), toggles)
check('toggle: custom control ends checked after click, Space, Space', (await page.getAttribute('button[role="switch"]:not([disabled])', 'aria-checked')) === 'true')
}

// Tabs primitives (packages/ui/src/demos/tabs.demo.tsx, tabs.test.tsx)
{
await open('ui/tabs')
await page.evaluate(() => {
  window.__tabs = []
  document.addEventListener('rmx:tabs-change', (e) => window.__tabs.push([e.activeTab, e.previousActiveTab, e.target.getAttribute('role')]))
})
const tabState = () => page.evaluate(() => ({
  selected: [...document.querySelectorAll('[role="tab"]')].map((t) => t.getAttribute('aria-selected')).join(),
  hidden: [...document.querySelectorAll('[role="tabpanel"]')].map((p) => p.hidden).join(),
  focus: document.activeElement?.textContent ?? '',
}))
const t0 = await tabState()
check('tabs: default tab active, others hidden', t0.selected === 'true,false,false' && t0.hidden === 'false,true,true', t0)
check('tabs: ids link tabs and panels', await page.evaluate(() => [...document.querySelectorAll('[role="tab"]')].every((t) => document.getElementById(t.getAttribute('aria-controls'))?.getAttribute('aria-labelledby') === t.id)))
await page.click('[role="tab"]:has-text("Activity")')
await settle()
const t1 = await tabState()
check('tabs: click activates a tab and its panel', t1.selected === 'false,true,false' && t1.hidden === 'true,false,true', t1)
await page.keyboard.press('ArrowRight')
await settle()
const t2 = await tabState()
check('tabs: ArrowRight skips the disabled tab and wraps, moving focus', t2.selected === 'true,false,false' && t2.focus === 'Overview', t2)
await page.keyboard.press('End')
await settle()
const t3 = await tabState()
check('tabs: End goes to the last enabled tab', t3.selected === 'false,true,false' && t3.focus === 'Activity', t3)
await page.keyboard.press('Home')
await settle()
const t4 = await tabState()
check('tabs: Home goes to the first tab', t4.selected === 'true,false,false' && t4.focus === 'Overview', t4)
await page.click('[role="tab"]:has-text("Settings")', { force: true })
await settle()
check('tabs: the disabled tab cannot be activated', (await tabState()).selected === 'true,false,false')
await page.focus('[role="tab"]:has-text("Activity")')
await page.keyboard.press('Enter')
await settle()
check('tabs: Enter activates the focused tab', (await tabState()).selected === 'false,true,false')
const tabEvents = await page.evaluate(() => window.__tabs)
check('tabs: rmx:tabs-change bubbles from the root with active and previous tabs', eq(tabEvents, [['activity', 'overview', null], ['overview', 'activity', null], ['activity', 'overview', null], ['overview', 'activity', null], ['activity', 'overview', null]]), tabEvents)
}

// Accordion primitives (packages/ui/src/demos/accordion.demo.tsx, accordion.test.tsx)
{
  await open('ui/accordion')
  await page.evaluate(() => {
    window.__acc = []
    document.addEventListener('rmx:accordion-change', (e) => window.__acc.push([e.accordionType, e.itemValue, e.value, e.target.dataset.type]))
  })
  const acc = () => page.evaluate(() => ({
    expanded: [...document.querySelectorAll('h3 button')].map((b) => b.getAttribute('aria-expanded')).join(),
    visible: [...document.querySelectorAll('[aria-labelledby]')].map((p) => getComputedStyle(p).display !== 'none').join(),
    focus: document.activeElement?.textContent ?? '',
  }))
  const a0 = await acc()
  check('accordion: default item open, disabled item closed', a0.expanded === 'true,false,false' && a0.visible === 'true,false,false', a0)
  check('accordion: triggers and panels are linked', await page.evaluate(() => [...document.querySelectorAll('h3 button')].every((b) => document.getElementById(b.getAttribute('aria-controls'))?.getAttribute('aria-labelledby') === b.id)))
  await page.click('h3 button:has-text("Billing")')
  await settle()
  const a1 = await acc()
  check('accordion: opening another item closes the first (single)', a1.expanded === 'false,true,false' && a1.visible === 'false,true,false', a1)
  await page.click('h3 button:has-text("Billing")')
  await settle()
  check('accordion: single items collapse by default', (await acc()).expanded === 'false,false,false')
  await page.click('h3 button:has-text("Archived")', { force: true })
  await settle()
  check('accordion: disabled item does not open', (await acc()).expanded === 'false,false,false')
  await page.focus('h3 button:has-text("Shipping")')
  await page.keyboard.press('ArrowDown')
  const f1 = (await acc()).focus
  await page.keyboard.press('ArrowDown')
  const f2 = (await acc()).focus
  await page.keyboard.press('End')
  const f3 = (await acc()).focus
  await page.keyboard.press('Home')
  const f4 = (await acc()).focus
  check('accordion: arrows wrap over enabled triggers, Home and End', [f1, f2, f3, f4].join() === 'Billing,Shipping,Billing,Shipping', [f1, f2, f3, f4])
  const events = await page.evaluate(() => window.__acc)
  check('accordion: rmx:accordion-change bubbles from the root', eq(events, [['single', 'billing', 'billing', 'single'], ['single', 'billing', null, 'single']]), events)
}

// Anchor (packages/ui/src/demos/anchor.demo.tsx)
{
  await open('ui/anchor')
  await page.waitForTimeout(50)
  const a = await page.evaluate(() => {
    const b = document.querySelector('button').getBoundingClientRect()
    const f = [...document.querySelectorAll('div')].find((d) => d.textContent === 'Positioned surface')
    const r = f.getBoundingClientRect()
    return { placement: f.dataset.anchorPlacement, dy: Math.round(r.top - b.bottom), dx: Math.round(r.left - b.left), position: f.style.position }
  })
  check('anchor: floating element sits 8px below the anchor, start-aligned', a.placement === 'bottom-start' && a.dy === 8 && a.dx === 0 && a.position === 'absolute', a)
  await page.setViewportSize({ width: 900, height: 720 })
  await page.waitForTimeout(100)
  const moved = await page.evaluate(() => {
    const b = document.querySelector('button').getBoundingClientRect()
    const r = [...document.querySelectorAll('div')].find((d) => d.textContent === 'Positioned surface').getBoundingClientRect()
    return { dy: Math.round(r.top - b.bottom), dx: Math.round(r.left - b.left) }
  })
  check('anchor: it follows the anchor when a resize moves it', moved.dy === 8 && moved.dx === 0, moved)
  await page.setViewportSize({ width: 1280, height: 720 })
}

// Popover (packages/ui/src/demos/popover.demo.tsx, popover.test.tsx)
{
  await open('ui/popover')
  const pop = () => page.evaluate(() => {
    const s = document.querySelector('[popover]')
    const b = document.querySelector('button').getBoundingClientRect()
    const r = s.getBoundingClientRect()
    return { open: s.matches(':popover-open'), focus: document.activeElement?.textContent, overflow: document.documentElement.style.overflow, placement: s.dataset.anchorPlacement, dy: Math.round(r.top - b.bottom), dx: Math.round(r.left - b.left) }
  })
  const p0 = await pop()
  check('popover: closed manual popover after hydration', !p0.open && p0.overflow === '', p0)
  await page.click('button:has-text("View options")')
  await settle()
  const p1 = await pop()
  check('popover: opens anchored below, focuses the show target, locks scroll', p1.open && p1.focus === 'Close' && p1.placement === 'bottom-start' && p1.dy === 8 && p1.dx === 0 && p1.overflow === 'hidden', p1)
  await page.keyboard.press('Escape')
  await settle()
  const p2 = await pop()
  check('popover: Escape hides, restores focus and scrolling', !p2.open && p2.focus === 'View options' && p2.overflow === '', p2)
  await page.click('button:has-text("View options")')
  await settle()
  await page.mouse.click(5, 5)
  await settle()
  check('popover: an outside click hides it', !(await pop()).open)
  await page.click('button:has-text("View options")')
  await settle()
  await page.click('button:has-text("View options")')
  await settle()
  check('popover: clicking the anchor while open closes it (the click does not reopen it)', !(await pop()).open)
  await page.click('button:has-text("View options")')
  await settle()
  await page.click('button:has-text("Close")')
  await settle()
  const p3 = await pop()
  check('popover: the close button hides it and focus returns to the anchor', !p3.open && p3.focus === 'View options', p3)
}

check('no page errors or failed requests', errors.length === 0, errors)
await browser.close()
console.log(failures ? `${failures} failed` : 'all passed')
process.exit(failures ? 1 : 0)
