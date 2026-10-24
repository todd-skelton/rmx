# Profiling image for the router: Rust plus valgrind. Build context: the repository root.
# docker build -f bench/docker/profile.Dockerfile -t rmxbench-profile .
FROM rust:1-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends valgrind && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release -p rmx --example router_profile && cp target/release/examples/router_profile /router_profile
CMD ["/router_profile"]
