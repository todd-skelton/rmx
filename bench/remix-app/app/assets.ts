import { createAssetServer } from 'remix/assets'

const isDevelopment = (process.env.NODE_ENV ?? 'development') === 'development'

export const assets = createAssetServer({
  basePath: '/assets',
  rootDir: process.cwd(),
  allowFiles: ['app/**/public/**'],
  allowPackages: ['remix'],
  sourceMaps: isDevelopment ? 'external' : undefined,
  minify: !isDevelopment,
  // Matches remix/template: file watching only in development.
  watch: isDevelopment,
})

export const scriptEntry = await assets.getScriptEntry('app/public/entry.ts')
