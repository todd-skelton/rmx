import { clientEntry, on, type Handle } from 'remix/component'

// The Remix 3 README counter.
export const Counter = clientEntry(
  import.meta.url,
  function Counter(handle: Handle<{ initial: number }>) {
    let count = handle.props.initial

    return () => (
      <button
        id="counter"
        type="button"
        mix={on('click', () => {
          count++
          handle.update()
        })}
      >
        Count: {count}
      </button>
    )
  },
)
