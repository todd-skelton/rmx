import { clientEntry, on, type Handle } from 'remix/component'

import { label } from './labels.ts'

interface Row {
  id: number
  label: string
}

export const RowsBench = clientEntry(
  import.meta.url,
  function RowsBench(handle: Handle) {
    let rows: Row[] = []
    let nextId = 0
    let seed = { value: 7 }
    let selected: number | undefined

    handle.queueTask(() => {
      document.documentElement.dataset.hydrated = String(performance.now())
    })

    function build(count: number): Row[] {
      let out: Row[] = []
      for (let i = 0; i < count; i++) out.push({ id: ++nextId, label: label(seed) })
      return out
    }

    // Run f, render, then record the elapsed time on <html data-last-op>.
    function op(name: string, f: () => void) {
      return on('click', () => {
        let start = performance.now()
        f()
        handle.update()
        handle.queueTask(() => {
          document.documentElement.dataset.lastOp = `${name}:${performance.now() - start}`
        })
      })
    }

    return () => (
      <div class="bench">
        <div class="controls">
          <button id="create" mix={op('create', () => (rows = build(1_000)))}>Create 1,000</button>
          <button id="create10k" mix={op('create10k', () => (rows = build(10_000)))}>Create 10,000</button>
          <button id="append" mix={op('append', () => (rows = rows.concat(build(1_000))))}>Append 1,000</button>
          <button
            id="update"
            mix={op('update', () => {
              for (let i = 0; i < rows.length; i += 10) rows[i].label += ' !!!'
            })}
          >
            Update every 10th
          </button>
          <button
            id="swap"
            mix={op('swap', () => {
              if (rows.length > 998) [rows[1], rows[998]] = [rows[998], rows[1]]
            })}
          >
            Swap rows
          </button>
          <button id="select" mix={op('select', () => (selected = rows[1]?.id))}>Select row 2</button>
          <button id="clear" mix={op('clear', () => (rows = []))}>Clear</button>
        </div>
        <table class="rows">
          <tbody>
            {rows.map((row) => (
              <tr key={row.id} class={selected === row.id ? 'danger' : undefined}>
                <td class="id">{row.id}</td>
                <td>
                  <a
                    class="select"
                    mix={on('click', () => {
                      selected = row.id
                      handle.update()
                    })}
                  >
                    {row.label}
                  </a>
                </td>
                <td>
                  <a
                    class="remove"
                    mix={on('click', () => {
                      rows = rows.filter((r) => r.id !== row.id)
                      handle.update()
                    })}
                  >
                    x
                  </a>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    )
  },
)
