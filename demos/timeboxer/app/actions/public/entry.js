// actions/public/entry.ts: start the runtime, which hydrates the client entries
// (ScheduleSidebar, ScheduleGrid) compiled into the wasm module.
document.addEventListener('rmx:ready', () => {
  document.documentElement.dataset.rmxReady = 'true'
})
const { default: init } = await import('./pkg/timeboxer_demo.js')
await init()
