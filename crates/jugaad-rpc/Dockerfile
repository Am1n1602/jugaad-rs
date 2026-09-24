# Build from the repo root so the whole Cargo workspace (jugaad-rpc
# depends on jugaad-core via a workspace path dependency) is in context:
#
#   docker build -f crates/jugaad-rpc/Dockerfile -t jugaad-rpc .
#   docker run --rm -p 50051:50051 jugaad-rpc

FROM rust:1.88-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p jugaad-rpc

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/jugaad-rpc /usr/local/bin/jugaad-rpc
EXPOSE 50051
ENTRYPOINT ["/usr/local/bin/jugaad-rpc"]
