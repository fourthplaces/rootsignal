FROM rust:1.93-slim-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
RUN cargo build --release --bin rootsignal-server --bin run-migrations

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 chromium && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/rootsignal-server /usr/local/bin/rootsignal-server
COPY --from=builder /app/target/release/run-migrations /usr/local/bin/run-migrations
COPY --from=builder /app/config /app/config
ENV CHROME_BIN=/usr/bin/chromium

WORKDIR /app
ENV PORT=9080
EXPOSE 9080 9081
CMD ["sh", "-c", "run-migrations && rootsignal-server"]
