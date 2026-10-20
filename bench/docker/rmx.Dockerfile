# rmx bench server for Linux. Build context: ../../rmx
# The browser bundle (examples/bench/public/pkg) is prebuilt by rmx/build.ps1 and copied as-is.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release -p bench-server && cp target/release/bench-server /bench-server

FROM debian:bookworm-slim
COPY --from=build /bench-server /usr/local/bin/bench-server
COPY examples/bench/public /app/public
ENV HOST=0.0.0.0 PORT=44101 RMX_PUBLIC=/app/public
EXPOSE 44101
CMD ["bench-server"]
