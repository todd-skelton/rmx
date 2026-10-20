# Load generator: same oha version as bench/tools/oha.exe.
FROM debian:bookworm-slim
ADD --chmod=755 https://github.com/hatoo/oha/releases/download/v1.16.0/oha-linux-amd64 /usr/local/bin/oha
ENTRYPOINT ["oha"]
