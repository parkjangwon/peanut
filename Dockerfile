FROM node:22-bookworm-slim AS console-builder
WORKDIR /app/console
COPY console/package*.json ./
RUN npm ci
COPY console/ ./
RUN npm run build

FROM rust:1-slim-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y pkg-config libssl-dev libsqlite3-dev build-essential && rm -rf /var/lib/apt/lists/*
COPY . .
COPY --from=console-builder /app/console/out ./console/out
RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=denoland/deno:bin /deno /usr/local/bin/deno
RUN apt-get update && apt-get install -y libssl3 libsqlite3-0 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/peanut ./peanut
RUN mkdir -p /app/data/storage
EXPOSE 3000
CMD ["./peanut"]
