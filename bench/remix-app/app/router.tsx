import { createRouter } from 'remix/router'
import { formData } from 'remix/middleware/form-data'
import { render } from 'remix/middleware/render'
import { staticFiles } from 'remix/middleware/static'
import { createRedirectResponse } from 'remix/response/redirect'

import { assets } from './assets.ts'
import { ClientPage, HomePage, SsrPage, type Task } from './pages.tsx'

const tasks: Task[] = [1, 2, 3, 4, 5].map((id) => ({ id, title: `Seed task ${id}` }))

export const router = createRouter({
  middleware: [staticFiles('./public', { index: false }), formData(), render({ assets })],
})

router.get('/assets/*path', async ({ request }) =>
  (await assets.fetch(request)) ?? new Response('Not Found', { status: 404 }),
)

router.get('/', (context) => context.render(<HomePage tasks={[...tasks]} />))

router.post('/tasks', ({ get }) => {
  let title = String(get(FormData)?.get('title') ?? '')
  tasks.push({ id: tasks.length + 1, title })
  return createRedirectResponse('/', 303)
})

router.get('/ssr/:n', (context) => {
  let n = Math.min(Number.parseInt(context.params.n, 10) || 0, 10_000)
  return context.render(<SsrPage n={n} />)
})

router.get('/client', (context) => context.render(<ClientPage />))
