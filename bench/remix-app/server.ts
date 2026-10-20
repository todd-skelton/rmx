// Same as the Remix 3 template's server.ts, minus HMR.
import * as http from 'node:http'
import { createRequestListener } from 'remix/node-fetch-server'

import { router } from './app/router.tsx'

const host = process.env.HOST ?? '127.0.0.1'
const port = process.env.PORT ? Number.parseInt(process.env.PORT, 10) : 44100

const server = http.createServer(createRequestListener(router.fetch))
server.listen(port, host, () => {
  console.log(`remix bench listening on http://${host}:${port}`)
})

function shutdown() {
  server.close(() => process.exit(0))
  server.closeAllConnections()
}
process.on('SIGINT', shutdown)
process.on('SIGTERM', shutdown)
