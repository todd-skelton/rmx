# rmx bench server for Linux. Build context: the repository root (../..)
# The browser bundle (bench/rmx-app/public/pkg) is prebuilt by scripts/build-bench.ps1 and copied as-is.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
# The target cache is shared by every build, including ones from older commits (the
# -RmxImage A/B runs), so touch the copied sources: cargo then rebuilds the workspace
# crates from what was copied rather than trusting artifacts with newer timestamps.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    find /src -path /src/target -prune -o -type f -exec touch {} + && \
    cargo build --release -p bench-server && cp target/release/bench-server /bench-server

FROM debian:bookworm-slim
COPY --from=build /bench-server /usr/local/bin/bench-server
COPY bench/rmx-app/public /app/public
ENV HOST=0.0.0.0 PORT=44101 RMX_PUBLIC=/app/public
EXPOSE 44101
CMD ["bench-server"]
