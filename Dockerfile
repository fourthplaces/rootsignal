FROM rust:1.83-slim-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
RUN cargo build --release --bin rootsignal-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/rootsignal-server /usr/local/bin/rootsignal-server
COPY --from=builder /app/migrations /app/migrations

WORKDIR /app
ENV PORT=9080
EXPOSE 9080 9081
CMD ["rootsignal-server"]
