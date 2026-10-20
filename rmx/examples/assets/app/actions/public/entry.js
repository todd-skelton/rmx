import { getContent } from './content.js'
import init, { mount } from './pkg/assets_demo.js'

const root = document.getElementById('app-root')

if (!(root instanceof HTMLElement)) {
  throw new Error('Expected #app-root to exist')
}

await init()
mount(root, getContent())
