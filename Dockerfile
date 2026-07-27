# syntax=docker/dockerfile:1

# ---- Build stage ----
FROM rust:1-slim-bookworm AS builder
WORKDIR /app

# rusqlite's "bundled" feature compiles SQLite from C source, so a C
# toolchain is required at build time (not at runtime).
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

# Build dependencies first so they're cached separately from source changes.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src

# static/index.html is embedded into the binary at compile time via
# include_str!, so it must be present for this build, but not in the final
# image.
COPY src ./src
COPY static ./static
# COPY can preserve mtimes that Cargo's fingerprinting considers "not newer"
# than the dummy build above, which would otherwise make it skip recompiling
# and silently ship the fn main() {} stub. Force fresh mtimes so the real
# source is always rebuilt.
RUN find src -type f -exec touch {} + && cargo build --release

# ---- Runtime stage ----
FROM debian:bookworm-slim AS runtime

RUN useradd --system --no-create-home --shell /usr/sbin/nologin linkrs
COPY --from=builder /app/target/release/linkrs /usr/local/bin/linkrs
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh \
    && mkdir -p /data \
    && chown linkrs:linkrs /data

WORKDIR /data

# Runs as root initially so the entrypoint can chown /data to match
# whatever gets mounted there (see docker-entrypoint.sh), then drops to the
# unprivileged linkrs user before ever executing app code. linkrs.db is
# created in /data on first run; mount a volume there to persist it (and the
# bootstrap admin account) across container restarts.
EXPOSE 3000
ENTRYPOINT ["docker-entrypoint.sh"]
