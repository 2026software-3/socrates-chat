-- 全站請求統計（F-39.2 / S-09.3）：每小時一列的合計，不含路由、使用者、IP 或 URL。
-- 耗時用固定分桶的直方圖（lat_Nms 是耗時 <= N 毫秒且大於前一個分桶上限的請求數），用來估 p95。
CREATE TABLE request_stats_hourly (
    hour timestamptz PRIMARY KEY,
    total bigint NOT NULL DEFAULT 0,
    status_2xx bigint NOT NULL DEFAULT 0,
    status_3xx bigint NOT NULL DEFAULT 0,
    status_4xx bigint NOT NULL DEFAULT 0,
    status_5xx bigint NOT NULL DEFAULT 0,
    lat_50ms bigint NOT NULL DEFAULT 0,
    lat_100ms bigint NOT NULL DEFAULT 0,
    lat_250ms bigint NOT NULL DEFAULT 0,
    lat_500ms bigint NOT NULL DEFAULT 0,
    lat_1000ms bigint NOT NULL DEFAULT 0,
    lat_2000ms bigint NOT NULL DEFAULT 0,
    lat_3000ms bigint NOT NULL DEFAULT 0,
    lat_5000ms bigint NOT NULL DEFAULT 0,
    lat_10000ms bigint NOT NULL DEFAULT 0,
    lat_over bigint NOT NULL DEFAULT 0
);
