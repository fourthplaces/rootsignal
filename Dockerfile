FROM rust:1.84-slim-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
RUN cargo build --release --bin web --bin scout

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/web /usr/local/bin/web
COPY --from=builder /app/target/release/scout /usr/local/bin/scout

WORKDIR /app
ENV WEB_HOST=0.0.0.0
ENV WEB_PORT=3000
EXPOSE 3000
CMD ["web"]
