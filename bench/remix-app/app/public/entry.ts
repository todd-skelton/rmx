import { run } from 'remix/component'

const app = run({
  async loadModule(moduleUrl, exportName) {
    let mod = await import(moduleUrl)
    return mod[exportName]
  },
})

await app.ready()
