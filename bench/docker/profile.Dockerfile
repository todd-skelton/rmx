# Profiling image for the router: Rust plus valgrind. Build context: the repository root.
# docker build -f bench/docker/profile.Dockerfile -t rsmixbench-profile .
FROM rust:1-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends valgrind && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
# The target cache is shared by every build, including ones from older commits (the
# -RsmixImage A/B runs), so touch the copied sources: cargo then rebuilds the workspace
# crates from what was copied rather than trusting artifacts with newer timestamps.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    find /src -path /src/target -prune -o -type f -exec touch {} + && \
    cargo build --release -p rsmix --example router_profile && cp target/release/examples/router_profile /router_profile
CMD ["/router_profile"]
