// Browser checks for the timeboxer demo: the ScheduleSidebar and ScheduleGrid client entries
// (create, edit, drag, resize, copy across days, delete) and the plain HTML auth flows.
// Start the server first (`SESSION_SECRET=dev cargo run -p timeboxer-demo`), then:
// node examples/timeboxer/e2e.mjs
// Env: BASE (http://127.0.0.1:44112), PLAYWRIGHT_NODE_MODULES, CHROMIUM_PATH, SHOT (screenshot path).
import { createRequire } from 'node:module'

const NODE_MODULES =
  process.env.PLAYWRIGHT_NODE_MODULES ??
  'C:/Users/ToddS/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/'
const { chromium } = createRequire(NODE_MODULES)('playwright')
const BASE = process.env.BASE ?? 'http://127.0.0.1:44112'

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
const username = `e2e-${Date.now().toString(36)}`

// --- No JavaScript: signup, bad login flash, login, account page, logout ---------------------
{
  const context = await browser.newContext({ baseURL: BASE, javaScriptEnabled: false })
  const page = await context.newPage()
  await attempt('signup validates and renders field errors', async () => {
    await page.goto('/auth/signup')
    await page.fill('input[name=username]', 'ab')
    await page.fill('input[name=password]', 'password123')
    // Skip the browser's own minlength check to reach the server's.
    await page.evaluate(() => document.querySelectorAll('input').forEach((i) => i.removeAttribute('minlength')))
    await page.click('button[type=submit]')
    await page.getByText('Expected at least 3 characters').waitFor({ timeout: 3000 })
    if ((await page.getAttribute('input[name=username]', 'aria-invalid')) !== 'true') throw new Error('aria-invalid missing')
  })
  await attempt('signup without JavaScript signs in and lands home', async () => {
    await page.fill('input[name=username]', username)
    await page.fill('input[name=password]', 'password123')
    await page.click('button[type=submit]')
    await page.getByText('Create a schedule to get started.').waitFor({ timeout: 3000 })
  })
  await attempt('logout, a flashed bad login, then login', async () => {
    await page.click('button:has-text("Logout")')
    await page.waitForURL('**/auth/login')
    await page.fill('input[name=username]', username)
    await page.fill('input[name=password]', 'wrong-password')
    await page.click('button[type=submit]')
    await page.locator('[role=alert]').getByText('Invalid username or password.').waitFor({ timeout: 3000 })
    await page.fill('input[name=username]', username)
    await page.fill('input[name=password]', 'password123')
    await page.click('button[type=submit]')
    await page.goto('/auth')
    await page.getByText(`Welcome back, ${username}.`).waitFor({ timeout: 3000 })
  })
  await context.close()
}

// --- The client entries ------------------------------------------------------------------------
{
  const context = await browser.newContext({ baseURL: BASE, viewport: { width: 1280, height: 900 } })
  const page = await context.newPage()
  const errors = []
  page.on('pageerror', (e) => errors.push(e.message))
  // The duplicate-name check expects one 409 from the API.
  page.on('console', (m) => m.type() === 'error' && !m.text().includes('409 (Conflict)') && errors.push(m.text()))
  const put = async (action) => {
    const [res] = await Promise.all([page.waitForResponse((r) => r.request().method() === 'PUT'), action()])
    if (res.status() !== 200) throw new Error(`PUT ${res.status()}`)
    return (await res.json()).schedule
  }
  const box = (name) => page.locator(`[data-schedule-block="true"][aria-label="${name}"]`).boundingBox()
  const cell = (label) => page.locator(`[aria-label="${label}"]`).boundingBox()

  await page.goto('/auth/login')
  await page.fill('input[name=username]', username)
  await page.fill('input[name=password]', 'password123')
  await Promise.all([page.waitForURL(BASE + '/'), page.click('button[type=submit]')])
  // The login was a soft navigation that reconciled <html>, dropping the ready marker.
  await page.goto('/')
  await ready(page)

  // Record element.animate() calls (rmx-ui animations go through WAAPI).
  const recordAnimations = () =>
    page.evaluate(() => {
      window.__animated = []
      const animate = Element.prototype.animate
      Element.prototype.animate = function (keyframes, options) {
        window.__animated.push(this.tagName.toLowerCase() + ':' + JSON.stringify(keyframes[0]))
        return animate.call(this, keyframes, options)
      }
    })
  const animated = () => page.evaluate(() => window.__animated)

  await attempt('new schedule shows Creating, then navigates to the schedule', async () => {
    await recordAnimations()
    await page.click('text=New schedule')
    const input = page.getByLabel('Schedule name')
    await input.waitFor()
    if ((await page.evaluate(() => document.activeElement?.getAttribute('aria-label'))) !== 'Schedule name') throw new Error('input not focused')
    await input.fill('Week one')
    await input.press('Enter')
    await page.getByText('Creating').waitFor({ timeout: 3000 })
    const calls = await animated()
    // The form animates out (from its natural styles) while "Creating" scales in.
    if (!calls.includes('form:{}') || !calls.some((c) => c.startsWith('div:') && c.includes('scale(0.5)'))) throw new Error(JSON.stringify(calls))
    await page.waitForURL(/\/schedules\/\d+$/, { timeout: 10000 })
    await page.locator('a[aria-current="page"]').getByText('Week one').waitFor()
  })
  await attempt('grid scrolls to 6:30am on load', async () => {
    const top = await page.evaluate(() => document.querySelector('section[aria-label="Weekly schedule"] > div:nth-child(3)').scrollTop)
    if (top < 900) throw new Error(`scrollTop ${top}`)
  })
  await attempt('click a cell, type a name, Enter saves the block', async () => {
    await page.click('[aria-label="Monday 9:00am"]')
    await page.getByLabel('New block name').waitFor()
    await page.keyboard.type('Deep work')
    const schedule = await put(() => page.keyboard.press('Enter'))
    const b = schedule.blocks[0]
    if (schedule.blocks.length !== 1 || b.name !== 'Deep work' || b.dayOfWeek !== 0 || b.startMinute !== 540 || b.endMinute !== 555) throw new Error(JSON.stringify(schedule.blocks))
  })
  const rowH = (await cell('Monday 9:15am')).y - (await cell('Monday 9:00am')).y
  const dayW = (await cell('Tuesday 9:00am')).x - (await cell('Monday 9:00am')).x
  await attempt('drag the bottom edge to resize', async () => {
    const b = await box('Deep work')
    await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2)
    const h = await page.locator('[aria-label="Deep work"] [aria-label="End resize handle"]').boundingBox()
    const schedule = await put(async () => {
      await page.mouse.move(h.x + h.width / 2, h.y + h.height / 2)
      await page.mouse.down()
      await page.mouse.move(h.x + h.width / 2, h.y + h.height / 2 + 3 * rowH, { steps: 8 })
      await page.mouse.up()
    })
    if (schedule.blocks[0].endMinute !== 600) throw new Error(JSON.stringify(schedule.blocks))
  })
  await attempt('drag a block to another day and time', async () => {
    const b = await box('Deep work')
    const schedule = await put(async () => {
      await page.mouse.move(b.x + b.width / 2, b.y + 10)
      await page.mouse.down()
      await page.mouse.move(b.x + b.width / 2 + dayW, b.y + 10 + 4 * rowH, { steps: 12 })
      if (!(await page.locator('section[data-schedule-dragging="true"]').count())) throw new Error('no drag state')
      await page.mouse.up()
    })
    const [b0] = schedule.blocks
    if (b0.dayOfWeek !== 1 || b0.startMinute !== 600 || b0.endMinute !== 660) throw new Error(JSON.stringify(schedule.blocks))
  })
  await attempt('drag a side edge to copy across days', async () => {
    const b = await box('Deep work')
    await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2)
    const h = await page.locator('[aria-label="Deep work"] [aria-label="Last day resize handle"]').boundingBox()
    const schedule = await put(async () => {
      await page.mouse.move(h.x + h.width / 2, h.y + h.height / 2)
      await page.mouse.down()
      await page.mouse.move(h.x + h.width / 2 + 2 * dayW, h.y + h.height / 2, { steps: 10 })
      await page.mouse.up()
    })
    const days = schedule.blocks.map((b) => b.dayOfWeek).join()
    if (days !== '1,2,3' || !schedule.blocks.every((b) => b.startMinute === 600)) throw new Error(JSON.stringify(schedule.blocks))
  })
  await attempt('moving onto a block pushes it out of the way', async () => {
    const blocks = page.locator('[data-schedule-block="true"]')
    const wed = await blocks.nth(1).boundingBox()
    await recordAnimations()
    const schedule = await put(async () => {
      await page.mouse.move(wed.x + wed.width / 2, wed.y + 10)
      await page.mouse.down()
      await page.mouse.move(wed.x + wed.width / 2 - dayW, wed.y + 10 + rowH, { steps: 10 })
      await page.mouse.up()
    })
    // Tuesday 10:15am overlaps the Tuesday block, which slides up to the nearest open slot.
    const tuesday = schedule.blocks.filter((b) => b.dayOfWeek === 1).map((b) => [b.startMinute, b.endMinute])
    if (JSON.stringify(tuesday) !== '[[555,615],[615,675]]') throw new Error(JSON.stringify(tuesday))
    // The pushed block slides to its preview position (animate_layout).
    const calls = await animated()
    if (!calls.some((c) => c.startsWith('div:') && c.includes('translate3d'))) throw new Error(JSON.stringify(calls))
  })
  await attempt('double-click renames; Backspace deletes the selected block', async () => {
    const first = page.locator('[data-schedule-block="true"]').first()
    await first.dblclick()
    await page.locator('[data-editing="true"]').waitFor()
    await page.keyboard.press('Control+A')
    await page.keyboard.type('Focus time')
    let schedule = await put(() => page.keyboard.press('Enter'))
    if (!schedule.blocks.some((b) => b.name === 'Focus time')) throw new Error(JSON.stringify(schedule.blocks))
    const count = schedule.blocks.length
    await page.locator('[data-schedule-block="true"]').last().click()
    schedule = await put(() => page.keyboard.press('Backspace'))
    if (schedule.blocks.length !== count - 1) throw new Error(JSON.stringify(schedule.blocks))
  })
  await attempt('the schedule survives a reload', async () => {
    await page.reload()
    await ready(page)
    await page.locator('[data-schedule-block="true"][aria-label="Focus time"]').waitFor({ timeout: 3000 })
  })
  await attempt('Download ICS returns the calendar', async () => {
    const href = await page.getByText('Download ICS').getAttribute('href')
    const res = await page.request.get(href)
    const body = await res.text()
    if (res.headers()['content-type'] !== 'text/calendar; charset=utf-8' || !body.includes('SUMMARY:Focus time\r\n')) throw new Error(body.slice(0, 200))
    if (res.headers()['content-disposition'] !== 'attachment; filename="week-one.ics"') throw new Error(res.headers()['content-disposition'])
  })
  await attempt('a duplicate schedule name shows the API error inline', async () => {
    await page.click('text=New schedule')
    await page.getByLabel('Schedule name').fill('Week one')
    await page.getByLabel('Schedule name').press('Enter')
    await page.locator('#new-schedule-name-error').getByText('Name must be unique.').waitFor({ timeout: 4000 })
    await page.keyboard.press('Escape')
    await page.getByRole('button', { name: 'New schedule' }).waitFor()
  })
  await attempt('deleting another schedule removes its row in place', async () => {
    await page.click('text=New schedule')
    await page.getByLabel('Schedule name').fill('Week two')
    await page.getByLabel('Schedule name').press('Enter')
    await page.locator('a[aria-current="page"]').getByText('Week two').waitFor({ timeout: 10000 })
    const url = page.url()
    await page.hover('a:has-text("Week one")')
    await page.click('button[aria-label="Delete Week one"]')
    await page.locator('a[data-deleting="true"]').waitFor({ timeout: 1000 })
    await page.locator('button[aria-label="Delete Week one"]').waitFor({ state: 'detached', timeout: 4000 })
    if (page.url() !== url) throw new Error(`navigated to ${page.url()}`)
  })
  await attempt('deleting the active schedule navigates to the next one', async () => {
    await page.hover('a:has-text("Week two")')
    await page.click('button[aria-label="Delete Week two"]')
    await page.waitForURL(BASE + '/', { timeout: 5000 })
    await page.getByText('Create a schedule to get started.').waitFor()
  })
  check('no page errors', errors.length === 0, errors)
  if (process.env.SHOT) await page.screenshot({ path: process.env.SHOT })
  await context.close()
}

await browser.close()
console.log(failures ? `${failures} check(s) failed` : 'all checks passed')
process.exit(failures ? 1 : 0)
