# 第三方授權盤點

本專案以 [MIT](../LICENSE) 授權。這份文件記錄所使用的開源套件的授權類型，以及選擇 MIT 的依據。盤點日期：2026-10-03，依據 `backend/Cargo.lock` 與 `frontend/package-lock.json`（含開發用套件）。

## 結論

所有直接與間接依賴都是寬鬆授權（permissive），沒有 GPL／AGPL 等強 copyleft，因此專案本身可以自由選擇授權；採用 MIT。需要留意的只有下方「需要注意的項目」。

## 後端（Rust crates，約 320 個）

| 授權（SPDX） | 說明 |
| --- | --- |
| `MIT`、`Apache-2.0`、`MIT OR Apache-2.0`（絕大多數） | 寬鬆授權 |
| `BSD-2-Clause`／`BSD-3-Clause`、`ISC`、`Zlib`、`Unlicense OR MIT`、`BSL-1.0` | 寬鬆授權 |
| `Unicode-3.0`、`CDLA-Permissive-2.0` | 寬鬆授權（資料表／憑證資料） |
| `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | 三選一，選 MIT 或 Apache-2.0 即可，不適用 LGPL |

## 前端（npm 套件，約 770 個）

| 授權（SPDX） | 說明 |
| --- | --- |
| `MIT`、`ISC`、`Apache-2.0`、`BSD-2-Clause`／`BSD-3-Clause`、`0BSD`、`MIT-0`、`BlueOak-1.0.0`、`CC0-1.0`、`Python-2.0` | 寬鬆授權 |
| `MIT OR CC0-1.0` | 二選一，選 MIT |

## 需要注意的項目

| 套件 | 授權 | 影響 |
| --- | --- | --- |
| `lightningcss`（Tailwind CSS v4 的建置工具） | MPL-2.0 | 只在建置時使用，不會被打包進交付給使用者的檔案。MPL-2.0 是檔案層級的 copyleft，沒有修改該套件就沒有義務。 |
| `@fontsource-variable/geist`（Geist 字型） | OFL-1.1 | 字型檔會被打包進前端建置產物。OFL 允許嵌入與散布，條件是保留版權與授權聲明（見下）；字型不可單獨販售。 |
| `caniuse-lite`（瀏覽器支援資料，建置時用） | CC-BY-4.0 | 只在建置時使用，不會散布。若日後散布其資料，需標示出處。 |

### Geist 字型聲明

Copyright 2024 The Geist Project Authors (https://github.com/vercel/geist-font)。

This Font Software is licensed under the SIL Open Font License, Version 1.1. 授權全文與 FAQ：https://openfontlicense.org 。完整文字也隨套件放在 `node_modules/@fontsource-variable/geist/LICENSE`。

## 維護

- 新增依賴前，先確認授權不是強 copyleft（GPL／AGPL）或不允許散布的授權。
- 重新盤點：後端 `cargo metadata --locked --format-version 1`（看每個套件的 `license`）；前端讀取 `frontend/package-lock.json` 每個套件的 `license`。
- 若要在 CI 強制檢查，可以另開 PR 加入 `cargo-deny`（後端）與對應的前端檢查；這會多一項工具，所以目前沒有加入。
