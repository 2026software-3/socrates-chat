# 單一映像：後端 API ＋ 前端建置產物，同網域提供（S-08.1）。
# 建置：docker compose --profile app up --build

FROM node:24-slim AS web
WORKDIR /web
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
# src/test/contract.ts 以相對路徑 import 後端的 OpenAPI 快照（../../../docs/api/openapi.json），
# 而 tsconfig.app.json 涵蓋整個 src，tsc -b 會去解析它；不放進來建置就失敗（2026-11-28 staging）。
COPY docs/api/openapi.json /docs/api/openapi.json
# 文字類產物預先壓成 .gz（保留原檔）：後端在瀏覽器接受 gzip 時直接送 .gz，執行期不必壓縮。
# 字型（woff2）本身已壓縮，不處理
# 最後一步把權限統一成目錄 755、檔案 644：後端以非 root 的 socrates 執行，讀不到 root 專屬（0600）的檔案時，
# 靜態檔服務會把「讀不到」當成「找不到」而退回 index.html。public/ 的檔案（favicon、語音的 AudioWorklet 腳本）
# 在建置時沿用來源檔的權限，而來源檔的權限取決於建置主機的 umask（部署腳本用 umask 077 做 git checkout，
# 結果是 0600），語音因此載入失敗（2026-11-20）。不要依賴主機的權限。
RUN npm run build \
    && find dist -type f \( -name '*.js' -o -name '*.css' -o -name '*.html' -o -name '*.svg' \) -exec gzip -9 -k {} + \
    && chmod -R u=rwX,go=rX dist

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
# 論證樹的提示與輸出 schema 以 include_str!("../eval/main_argument_tree/…") 編進執行檔
COPY backend/eval/main_argument_tree ./eval/main_argument_tree
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
