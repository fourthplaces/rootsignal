# -- Stage 1: compute recipe (dependency lockfile) --
FROM rust:1.89-slim-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

# -- Stage 2: capture dependency recipe --
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# -- Stage 3: build dependencies (cached unless Cargo.toml/lock change) --
FROM chef AS builder
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# -- Stage 4: build the api binary --
COPY . .
RUN cargo build --release --bin api

# -- Stage 5: minimal runtime image --
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 chromium && rm -rf /var/lib/apt/lists/*
ENV CHROME_BIN=/usr/bin/chromium

COPY --from=builder /app/target/release/api /usr/local/bin/api

ENV API_HOST=0.0.0.0
EXPOSE 3000
CMD ["api"]
