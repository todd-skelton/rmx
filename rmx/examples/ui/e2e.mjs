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
check('the index lists the demos', links.length === 16, links)

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

// Listbox (packages/ui/src/demos/listbox.demo.tsx, listbox.test.tsx)
{
  await open('ui/listbox')
  const lb = () => page.evaluate(() => ({
    selected: [...document.querySelectorAll('[role="option"]')].map((o) => o.getAttribute('aria-selected')).join(),
    highlighted: [...document.querySelectorAll('[role="option"]')].map((o) => o.dataset.highlighted).join(),
    value: [...document.querySelectorAll('p')].pop().textContent,
  }))
  const l0 = await lb()
  check('listbox: initial value and highlight', l0.selected === 'true,false,false,false' && l0.highlighted === 'true,false,false,false' && l0.value === 'value=remix', l0)
  await page.focus('[role="listbox"]')
  await page.keyboard.press('ArrowDown')
  await page.keyboard.press('ArrowDown')
  await settle()
  check('listbox: arrows move the highlight', (await lb()).highlighted === 'false,false,true,false')
  await page.keyboard.press('ArrowDown')
  await settle()
  check('listbox: navigation wraps past the disabled option', (await lb()).highlighted === 'true,false,false,false')
  await page.keyboard.press('End')
  await page.keyboard.press('Enter')
  await page.waitForTimeout(120)
  const l1 = await lb()
  check('listbox: End and Enter select the last enabled option', l1.selected === 'false,false,true,false' && l1.value === 'value=react', l1)
  await page.keyboard.press('Home')
  await page.keyboard.type('r')
  await settle()
  const t1 = (await lb()).highlighted
  await page.keyboard.type('e')
  await settle()
  const t2 = (await lb()).highlighted
  check('listbox: typeahead searches from the highlighted option', t1 === 'false,true,false,false' && t2 === 'false,false,true,false', [t1, t2])
  await page.waitForTimeout(800)
  await page.hover('[role="option"]:has-text("Remix")')
  await settle()
  check('listbox: hovering highlights', (await lb()).highlighted === 'true,false,false,false')
  await page.click('[role="option"]:has-text("Archived")', { force: true })
  await page.click('[role="option"]:has-text("Remix")')
  await page.waitForTimeout(120)
  const l2 = await lb()
  check('listbox: clicking selects; disabled options are ignored', l2.selected === 'true,false,false,false' && l2.value === 'value=remix', l2)
}

// Select (packages/ui/src/demos/select.demo.tsx, select.test.tsx)
{
  await open('ui/select')
  await page.evaluate(() => {
    window.__sel = []
    document.addEventListener('rmx:select-change', (e) => window.__sel.push([e.value, e.label, !!e.optionId, e.target.tagName]))
  })
  const sel = () => page.evaluate(() => {
    const t = document.querySelector('[aria-haspopup="listbox"]')
    const s = document.querySelector('[popover]')
    return {
      label: t.textContent, expanded: t.getAttribute('aria-expanded'), describedby: t.getAttribute('aria-describedby'),
      open: s.matches(':popover-open'), focus: document.activeElement?.getAttribute('role') ?? document.activeElement?.tagName,
      selected: [...document.querySelectorAll('[role="option"]')].map((o) => o.getAttribute('aria-selected')).join(),
      input: document.querySelector('input[type="hidden"]').value, value: [...document.querySelectorAll('p')].pop().textContent,
      minWidth: s.style.minWidth, triggerWidth: t.offsetWidth,
    }
  })
  await page.waitForTimeout(50)
  const s0 = await sel()
  const selectedId = await page.evaluate(() => document.querySelector('[role="option"][aria-selected="true"]').id)
  check('select: default value selected, default label on the trigger, aria-describedby wired', s0.label === 'Choose framework' && s0.selected === 'true,false,false,false' && s0.describedby === selectedId && s0.input === 'remix', { s0, selectedId })
  await page.click('[aria-haspopup="listbox"]')
  await settle()
  const s1 = await sel()
  check('select: click opens, syncs min-width and moves focus into the list', s1.open && s1.expanded === 'true' && s1.focus === 'listbox' && s1.minWidth === s1.triggerWidth + 'px', s1)
  await page.keyboard.press('Escape')
  await settle()
  const s2 = await sel()
  check('select: Escape closes and restores focus to the trigger', !s2.open && s2.focus === 'BUTTON', s2)
  await page.keyboard.press('ArrowDown')
  await settle()
  await page.keyboard.press('ArrowDown')
  await page.keyboard.press('Enter')
  await page.waitForTimeout(40)
  const flashing = await page.evaluate(() => document.querySelector('[data-select-flash]')?.textContent)
  await page.waitForTimeout(80)
  const s3 = await sel()
  await page.waitForTimeout(300)
  const s4 = await sel()
  check('select: ArrowDown reopens; Enter flashes the option and closes', flashing === 'React Router' && !s3.open, { flashing, s3 })
  check('select: the label swaps after the delay and the change event fires', s3.label === 'Choose framework' && s4.label === 'React Router' && s4.value === 'value=react-router' && s4.input === 'react-router', { s3, s4 })
  await page.keyboard.type('r')
  await page.waitForTimeout(100)
  const s5 = await sel()
  check('select: typeahead on the closed trigger selects immediately', !s5.open && s5.label === 'React' && s5.input === 'react' && s5.value === 'value=react', s5)
  const events = await page.evaluate(() => window.__sel)
  check('select: rmx:select-change bubbles from the trigger', eq(events, [['react-router', 'React Router', true, 'BUTTON'], ['react', 'React', true, 'BUTTON']]), events)
}

// Combobox (packages/ui/src/demos/combobox.demo.tsx, combobox.test.tsx)
{
  await open('ui/combobox')
  await page.evaluate(() => {
    window.__cb = []
    document.addEventListener('rmx:combobox-change', (e) => window.__cb.push([e.value, e.label, !!e.optionId, e.target.tagName]))
  })
  const input = '[role="combobox"]'
  const cb = () => page.evaluate(() => {
    const i = document.querySelector('[role="combobox"]')
    const s = document.querySelector('[popover]')
    const active = i.getAttribute('aria-activedescendant')
    return {
      text: i.value, expanded: i.getAttribute('aria-expanded'), open: s.matches(':popover-open'), reason: s.dataset.showReason,
      focus: document.activeElement === i, active: active ? document.getElementById(active)?.textContent : null,
      visible: [...document.querySelectorAll('[role="option"]')].filter((o) => !o.hidden).map((o) => o.textContent).join(),
      selectedText: i.value.slice(i.selectionStart, i.selectionEnd),
      input: document.querySelector('input[type="hidden"]').value, value: [...document.querySelectorAll('p')].pop().textContent,
      minWidth: s.style.minWidth, inputWidth: i.offsetWidth,
    }
  })
  await settle()
  const c0 = await cb()
  check('combobox: starts closed and empty', c0.text === '' && c0.expanded === 'false' && !c0.open && c0.input === '' && c0.value === 'value=none', c0)
  await page.focus(input)
  await page.keyboard.type('re')
  await settle()
  const c1 = await cb()
  check('combobox: typing opens a filtered hint list and keeps focus on the input', c1.text === 're' && c1.open && c1.reason === 'hint' && c1.focus && c1.active === null && c1.visible === 'Remix,React Router,React' && c1.minWidth === c1.inputWidth + 'px', c1)
  await page.keyboard.press('Enter')
  await settle()
  const c2 = await cb()
  check('combobox: Enter does nothing while typing has opened the list with no active option', c2.open && c2.value === 'value=none' && c2.text === 're', c2)
  await page.keyboard.press('ArrowDown')
  await settle()
  const c3 = await cb()
  await page.keyboard.press('ArrowDown')
  await settle()
  const c4 = await cb()
  check('combobox: ArrowDown navigates the active descendant in nav mode while the input stays focused', c3.active === 'Remix' && c4.active === 'React Router' && c4.reason === 'nav' && c4.focus, { c3, c4 })
  await page.keyboard.press('Enter')
  await page.waitForTimeout(30)
  const flashing = await page.evaluate(() => document.querySelector('[data-combobox-flash]')?.textContent)
  await page.waitForTimeout(400)
  const c5 = await cb()
  check('combobox: Enter flashes the option, closes, commits the label and selects the input text', flashing === 'React Router' && !c5.open && c5.text === 'React Router' && c5.selectedText === 'React Router' && c5.input === 'react-router' && c5.value === 'value=react-router' && c5.focus, { flashing, c5 })
  await page.keyboard.type('x')
  await settle()
  const c6 = await cb()
  check('combobox: typing clears the committed selection; no matches keeps the list closed', c6.text === 'x' && !c6.open && c6.input === '' && c6.value === 'value=none', c6)
  await page.keyboard.press('Escape')
  await settle()
  const c7 = await cb()
  check('combobox: Escape with a non-matching value clears the input', c7.text === '' && !c7.open, c7)
  await page.keyboard.type('react')
  await settle()
  const c8 = await cb()
  await page.evaluate(() => document.activeElement.blur())
  await page.waitForTimeout(300)
  const c9 = await cb()
  check('combobox: blur commits an exact match without rewriting the input text', c8.visible === 'React Router,React' && !c9.open && c9.text === 'react' && c9.input === 'react' && c9.value === 'value=react', { c8, c9 })
  await page.click(input)
  await settle()
  const c10 = await cb()
  check('combobox: clicking the input opens the full list with the selected option active', c10.open && c10.reason === 'nav' && c10.active === 'React' && c10.visible === 'Remix,React Router,React,Archived' && c10.focus, c10)
  await page.click('[role="option"] >> text=Remix')
  await page.waitForTimeout(400)
  const c11 = await cb()
  check('combobox: pointer selection keeps focus on the input', !c11.open && c11.focus && c11.text === 'Remix' && c11.input === 'remix', c11)
  await page.keyboard.press('ArrowDown')
  await settle()
  const c12 = await cb()
  check('combobox: ArrowDown on an exact-match closed input opens the unfiltered list', c12.open && c12.active === 'Remix' && c12.visible === 'Remix,React Router,React,Archived' && c12.focus, c12)
  await page.keyboard.press('Escape')
  await page.waitForTimeout(300)
  await page.fill(input, '')
  await page.keyboard.type('rmx')
  await settle()
  const c13 = await cb()
  await page.fill(input, '')
  await page.keyboard.type('rou')
  await settle()
  const c14 = await cb()
  check('combobox: searchValue strings filter options; disabled options are skipped', c13.visible === 'Remix' && c14.visible === 'React Router', { c13, c14 })
  const events = await page.evaluate(() => window.__cb)
  check('combobox: rmx:combobox-change bubbles from the input', eq(events, [
    ['react-router', 'React Router', true, 'INPUT'], [null, null, false, 'INPUT'], ['react', 'React', true, 'INPUT'], ['remix', 'Remix', true, 'INPUT'], [null, null, false, 'INPUT'],
  ]), events)
}

// Menu (packages/ui/src/demos/menu.demo.tsx, menu.test.tsx)
{
  await open('ui/menu')
  await page.evaluate(() => {
    window.__menu = []
    document.addEventListener('rmx:menu-select', (e) => window.__menu.push([e.item.name, e.item.value, e.item.checked ?? null, e.target.getAttribute('role')]))
  })
  const mstate = (trigger) => page.evaluate((trigger) => {
    const t = document.getElementById(trigger)
    const list = document.getElementById(t.getAttribute('aria-controls'))
    const a = document.activeElement
    return {
      expanded: t.getAttribute('aria-expanded'), open: list.closest('[popover]').matches(':popover-open'),
      focus: a === list ? 'list' : a?.id === trigger ? 'trigger' : a?.textContent ?? null,
      highlighted: [...list.querySelectorAll(':scope > [data-highlighted="true"]')].map((i) => i.textContent).join(),
      selected: [...document.querySelectorAll('p')].pop().textContent,
    }
  }, trigger)
  await page.click('#actions')
  await settle()
  const m0 = await mstate('actions')
  await page.click('#actions')
  await page.waitForTimeout(100)
  const m1 = await mstate('actions')
  check('menu: click opens with focus on the list; clicking the trigger again closes and refocuses it', m0.open && m0.expanded === 'true' && m0.focus === 'list' && !m1.open && m1.expanded === 'false' && m1.focus === 'trigger', { m0, m1 })
  await page.keyboard.press('ArrowDown')
  await settle()
  const m2 = await mstate('actions')
  const keys = []
  for (const key of ['ArrowDown', 'ArrowDown', 'Home', 'End', 'ArrowUp', 'ArrowUp']) {
    await page.keyboard.press(key)
    await settle()
    keys.push((await mstate('actions')).focus)
  }
  check('menu: ArrowDown opens at the first item; navigation skips disabled items and does not wrap', m2.open && m2.focus === 'Rename' && m2.highlighted === 'Rename' && eq(keys, ['Duplicate', 'Duplicate', 'Rename', 'Duplicate', 'Rename', 'Rename']), { m2, keys })
  await page.keyboard.type('d')
  await settle()
  const m3 = await mstate('actions')
  check('menu: typeahead highlights and focuses the matching item', m3.focus === 'Duplicate' && m3.highlighted === 'Duplicate', m3)
  await page.keyboard.press('Enter')
  await page.waitForTimeout(20)
  const flashing = await page.evaluate(() => document.querySelector('[data-menu-flash="true"]')?.textContent)
  await page.waitForTimeout(250)
  const m4 = await mstate('actions')
  check('menu: Enter flashes the item, dispatches the selection, closes and refocuses the trigger', flashing === 'Duplicate' && !m4.open && m4.focus === 'trigger' && m4.selected === 'selected=duplicate', { flashing, m4 })
  await page.keyboard.press('ArrowUp')
  await settle()
  const m5 = await mstate('actions')
  await page.keyboard.press('Escape')
  await page.waitForTimeout(100)
  const m6 = await mstate('actions')
  check('menu: ArrowUp opens at the last enabled item; Escape closes and refocuses the trigger', m5.open && m5.focus === 'Duplicate' && !m6.open && m6.focus === 'trigger', { m5, m6 })

  // Submenus
  await page.focus('#view')
  await page.keyboard.press('ArrowDown')
  await settle()
  await page.keyboard.press('ArrowDown')
  await settle()
  const v0 = await mstate('view')
  await page.keyboard.press('ArrowRight')
  await settle()
  const z0 = await mstate('zoom')
  await page.keyboard.press('ArrowLeft')
  await settle()
  const z1 = await mstate('zoom')
  await page.waitForTimeout(400)
  const z2 = await mstate('zoom')
  check('menu: ArrowRight opens the submenu at its first item; ArrowLeft closes it, refocuses its trigger and it stays closed', v0.focus === 'Zoom' && z0.open && z0.expanded === 'true' && z0.focus === 'Zoom in' && !z1.open && z1.focus === 'trigger' && !z2.open, { v0, z0, z1, z2 })
  const placement = await page.evaluate(() => {
    const trigger = document.getElementById('zoom').getBoundingClientRect()
    const surface = document.getElementById(document.getElementById('zoom').getAttribute('aria-controls')).closest('[popover]')
    return surface.dataset.anchorPlacement
  })
  await page.click('#zoom')
  await settle()
  const z3 = await mstate('zoom')
  const geometry = await page.evaluate(() => {
    const trigger = document.getElementById('zoom').getBoundingClientRect()
    const first = document.querySelector('#' + CSS.escape(document.getElementById('zoom').getAttribute('aria-controls')) + ' > [role="menuitem"]').getBoundingClientRect()
    const surface = document.getElementById(document.getElementById('zoom').getAttribute('aria-controls')).closest('[popover]')
    return { placement: surface.dataset.anchorPlacement, dy: Math.round(first.top - trigger.top), right: first.left >= trigger.right - 1 }
  })
  check('menu: clicking a submenu trigger opens it without moving focus, anchored right-start to its first item', z3.open && z3.focus === 'trigger' && geometry.placement === 'right-start' && geometry.dy === 0 && geometry.right, { z3, geometry, placement })
  await page.keyboard.press('ArrowRight')
  await settle()
  await page.keyboard.press('End')
  await settle()
  await page.keyboard.press('Enter')
  await page.waitForTimeout(300)
  const z4 = await mstate('zoom')
  const v1 = await mstate('view')
  check('menu: selecting a submenu item closes the whole tree and refocuses the root trigger', !z4.open && !v1.open && v1.focus === 'trigger' && v1.selected === 'selected=zoomOut:zoom-out', { z4, v1 })

  // Checkbox items and pointer highlighting
  await page.click('#view')
  await settle()
  const wrap = await page.$('#' + (await page.getAttribute('#view', 'aria-controls')) + ' > [role="menuitemcheckbox"]')
  const box = await wrap.boundingBox()
  await page.mouse.move(box.x + 10, box.y + box.height / 2)
  await page.mouse.move(box.x + 12, box.y + box.height / 2)
  await settle()
  const h0 = await mstate('view')
  await page.mouse.click(box.x + 12, box.y + box.height / 2)
  await page.waitForTimeout(20)
  const flashChecked = await page.evaluate(() => document.querySelector('[data-menu-flash="true"]')?.getAttribute('aria-checked'))
  await page.waitForTimeout(250)
  const checkedAfter = await page.evaluate(() => document.querySelector('[role="menuitemcheckbox"]').getAttribute('aria-checked'))
  check('menu: pointer movement highlights and focuses items', h0.highlighted === 'Word wrap' && h0.focus === 'Word wrap', h0)
  check('menu: a checkbox item flashes its committed checked state', flashChecked === 'true' && checkedAfter === 'true', { flashChecked, checkedAfter })

  // Hover opens a submenu after the delay
  await page.click('#view')
  await settle()
  const zb = await (await page.$('#zoom')).boundingBox()
  await page.mouse.move(zb.x + 10, zb.y + zb.height / 2)
  await page.mouse.move(zb.x + 14, zb.y + zb.height / 2)
  await page.waitForTimeout(100)
  const hv0 = await mstate('zoom')
  await page.waitForTimeout(350)
  const hv1 = await mstate('zoom')
  check('menu: hovering a submenu trigger opens the submenu after the delay', !hv0.open && hv1.open && hv1.expanded === 'true', { hv0, hv1 })
  await page.keyboard.press('Escape')
  await page.waitForTimeout(150)
  const hv2 = await mstate('view')
  check('menu: Escape in a submenu closes the whole tree', !hv2.open && !(await mstate('zoom')).open && hv2.focus === 'trigger', hv2)

  // Context menu
  const target = await (await page.$('#context-target')).boundingBox()
  await page.mouse.click(target.x + 20, target.y + 10, { button: 'right' })
  await settle()
  const pos = () => page.evaluate(() => {
    const surface = document.getElementById(document.getElementById('context-target').getAttribute('aria-controls')).closest('[popover]')
    const r = surface.getBoundingClientRect()
    return { open: surface.matches(':popover-open'), left: Math.round(r.left), top: Math.round(r.top), focus: document.activeElement?.getAttribute('role') }
  })
  const near = (a, b) => Math.abs(a - b) <= 1
  const c0 = await pos()
  await page.mouse.click(target.x + 6, target.y + 4, { button: 'right' })
  await page.waitForTimeout(100)
  const c1 = await pos()
  check('menu: contextmenu opens at the pointer; a second right click repositions it', c0.open && c0.focus === 'menu' && near(c0.left, target.x + 20) && near(c0.top, target.y + 10) && c1.open && near(c1.left, target.x + 6) && near(c1.top, target.y + 4), { target, c0, c1 })
  await page.keyboard.press('Escape')
  await page.waitForTimeout(100)
  await page.focus('#context-target')
  await page.keyboard.press('Shift+F10')
  await settle()
  const c2 = await pos()
  check('menu: Shift+F10 opens the context menu at the target bounds', c2.open && near(c2.left, target.x) && near(c2.top, target.y + target.height), { target, c2 })
  await page.keyboard.press('Tab')
  await page.waitForTimeout(150)
  const c3 = await pos()
  check('menu: Tab closes the menu', !c3.open, c3)

  const events = await page.evaluate(() => window.__menu)
  check('menu: one bubbling rmx:menu-select per selection, from the item', eq(events, [['duplicate', null, null, 'menuitem'], ['zoomOut', 'zoom-out', null, 'menuitem'], ['wordWrap', null, true, 'menuitemcheckbox']]), events)
}

check('no page errors or failed requests', errors.length === 0, errors)
await browser.close()
console.log(failures ? `${failures} failed` : 'all passed')
process.exit(failures ? 1 : 0)
