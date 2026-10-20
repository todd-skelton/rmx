import type { Handle, RemixNode } from 'remix/component'
import { ImportMap } from 'remix/component/server'

import { scriptEntry } from './assets.ts'
import { Counter } from './public/counter.tsx'
import { label } from './public/labels.ts'
import { RowsBench } from './public/rows-bench.tsx'

export interface Task {
  id: number
  title: string
}

function Document(handle: Handle<{ title: string; children?: RemixNode }>) {
  return () => {
    let { href, importMap, preloads } = scriptEntry
    return (
      <html lang="en">
        <head>
          <meta charSet="utf-8" />
          <meta name="viewport" content="width=device-width, initial-scale=1" />
          <title>{handle.props.title}</title>
          <link rel="stylesheet" href="/app.css" />
          <ImportMap value={importMap} />
          {preloads.map((preloadHref) => (
            <link key={preloadHref} rel="modulepreload" href={preloadHref} />
          ))}
          <script type="module" src={href}></script>
        </head>
        <body>
          <nav>
            <a href="/">Tasks</a>
            {' | '}
            <a href="/ssr/100">SSR rows</a>
            {' | '}
            <a href="/client">Client bench</a>
          </nav>
          <main>{handle.props.children}</main>
        </body>
      </html>
    )
  }
}

export function HomePage(handle: Handle<{ tasks: Task[] }>) {
  return () => {
    let { tasks } = handle.props
    return (
      <Document title="Tasks">
        <h1>Tasks</h1>
        <ul class="tasks">
          {tasks.map((t) => (
            <li key={t.id}>{t.title}</li>
          ))}
        </ul>
        <form method="post" action="/tasks">
          <input name="title" placeholder="New task" required />
          <button type="submit">Add</button>
        </form>
        <Counter initial={tasks.length} />
      </Document>
    )
  }
}

export function SsrPage(handle: Handle<{ n: number }>) {
  return () => {
    let { n } = handle.props
    let seed = { value: 42 }
    let rows = []
    for (let id = 1; id <= n; id++) {
      rows.push(<RowView key={id} id={id} label={label(seed)} done={id % 3 === 0} />)
    }
    return (
      <Document title="SSR rows">
        <h1>Rows: {n}</h1>
        <table class="rows">
          <tbody>{rows}</tbody>
        </table>
      </Document>
    )
  }
}

function RowView(handle: Handle<{ id: number; label: string; done: boolean }>) {
  return () => {
    let p = handle.props
    return (
      <tr class={p.done ? 'done' : undefined}>
        <td class="id">{p.id}</td>
        <td>
          <a href={`/rows/${p.id}`}>{p.label}</a>
        </td>
        <td>
          <span class="badge">{p.done ? 'done' : 'open'}</span>
        </td>
      </tr>
    )
  }
}

export function ClientPage() {
  return () => (
    <Document title="Client bench">
      <h1>Client bench</h1>
      <RowsBench />
    </Document>
  )
}
