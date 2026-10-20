# Remix 3 bench app. Build context: ../remix-app
FROM node:24-bookworm-slim
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci --omit=dev
COPY . .
ENV NODE_ENV=production HOST=0.0.0.0 PORT=44100
EXPOSE 44100
# Swap to cluster.ts (with REMIX_WORKERS) for the multi-process config.
CMD ["node", "--import", "remix/node-tsx", "server.ts"]
