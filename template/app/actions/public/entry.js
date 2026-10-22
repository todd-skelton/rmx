// app/actions/public/entry.ts: start the runtime, which hydrates the PromptButton client
// entries compiled into the wasm module.
document.addEventListener('rmx:ready', () => {
  document.documentElement.dataset.rmxReady = 'true'
})
const { default: init } = await import('./pkg/template_demo.js')
await init()
