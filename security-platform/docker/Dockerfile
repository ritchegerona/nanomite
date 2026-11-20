FROM rust:1.78-slim AS builder

WORKDIR /app
COPY . .
RUN cargo build --release --bin main

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/main /app/main

EXPOSE 8080
CMD ["/app/main"]