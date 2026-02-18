FROM rust:1.89-slim-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
RUN cargo build --release --bin api --bin scout

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/scout /usr/local/bin/scout
COPY --from=builder /app/target/release/api /usr/local/bin/api

WORKDIR /app
ENV WEB_HOST=0.0.0.0
ENV WEB_PORT=3000
EXPOSE 3000
CMD ["api"]
