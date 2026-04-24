# Build stage
FROM node:24-slim AS frontend-builder
WORKDIR /app/peanut-console
COPY peanut-console/package*.json ./
RUN npm install
COPY peanut-console/ .
RUN npx next build --webpack

FROM rust:1.81-slim AS backend-builder
WORKDIR /app
# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev libsqlite3-dev build-essential && rm -rf /var/lib/apt/lists/*
COPY . .
# Copy built frontend assets from previous stage
COPY --from=frontend-builder /app/peanut-console/out ./peanut-console/out
RUN cargo build --release

# Final stage
FROM debian:bookworm-slim
WORKDIR /app
RUN apt-get update && apt-get install -y libssl3 libsqlite3-0 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=backend-builder /app/target/release/peanut .
# Create data directory for SQLite and storage
RUN mkdir -p /app/data/storage
EXPOSE 3000
CMD ["./peanut"]
