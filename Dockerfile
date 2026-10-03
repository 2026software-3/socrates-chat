# 單一映像：後端 API ＋ 前端建置產物，同網域提供（S-08.1）。
# 建置：docker compose --profile app up --build

FROM node:24-slim AS web
WORKDIR /web
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

FROM rust:1-slim-bookworm AS api
WORKDIR /api
# 映像已內建 Rust；不複製 rust-toolchain.toml，免得 rustup 在容器內重新下載工具鏈
COPY backend/Cargo.toml backend/Cargo.lock ./
# 先用空殼原始碼編譯依賴，讓這一層在 Cargo.toml／Cargo.lock 沒變時可被快取
RUN mkdir src \
    && echo "fn main() {}" > src/main.rs \
    && touch src/lib.rs \
    && cargo build --release --locked \
    && rm -rf src
COPY backend/src ./src
COPY backend/migrations ./migrations
# 確保 cargo 重新編譯真正的原始碼，而非沿用空殼的產物
RUN touch src/main.rs src/lib.rs && cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home socrates
COPY --from=api /api/target/release/socrates-chat-backend /usr/local/bin/socrates-chat-backend
COPY --from=web /web/dist /app/dist
USER socrates
ENV LISTEN_ADDR=0.0.0.0:3000 FRONTEND_DIR=/app/dist
EXPOSE 3000
CMD ["socrates-chat-backend"]
