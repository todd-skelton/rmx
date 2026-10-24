// Multi-process variant of server.ts for a like-for-like comparison with rsmix's multithreaded
// server: one Node worker per logical core (override with REMIX_WORKERS), sharing the port.
import cluster from 'node:cluster'
import * as os from 'node:os'

if (cluster.isPrimary) {
  let workers = Number.parseInt(process.env.REMIX_WORKERS ?? '', 10) || os.availableParallelism()
  for (let i = 0; i < workers; i++) cluster.fork()
  let shutdown = () => {
    for (let w of Object.values(cluster.workers ?? {})) w?.kill()
    process.exit(0)
  }
  process.on('SIGINT', shutdown)
  process.on('SIGTERM', shutdown)
} else {
  await import('./server.ts')
}
