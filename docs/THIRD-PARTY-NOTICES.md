# 第三方聲明

本專案以 [MIT](../LICENSE) 授權。所有依賴（Rust crates、npm 套件）皆為寬鬆授權，沒有 GPL／AGPL 等強 copyleft（盤點於 2026-10-03，依據 `Cargo.lock` 與 `package-lock.json`）。

以下是必須保留的聲明：被複製進本 repo 的程式碼，與會被打包進建置產物的字型。

## 複製進原始碼的程式碼

shadcn/ui 與 assistant-ui 的元件原始碼被複製進本 repo（並依需求修改），MIT 要求保留原作者的版權與授權聲明。

| 來源 | 本專案中的位置 | 版權 |
| --- | --- | --- |
| [shadcn/ui](https://github.com/shadcn-ui/ui) | `frontend/src/components/ui/`、`frontend/src/lib/utils.ts` | Copyright (c) 2023 shadcn |
| [assistant-ui](https://github.com/assistant-ui/assistant-ui) | `frontend/src/components/assistant-ui/` | Copyright (c) 2026 AgentbaseAI Inc. |

兩者皆使用相同的 MIT 條款，授權文字如下（`<版權人>` 依上表代入）：

```text
MIT License

Copyright (c) <版權人>

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## 打包進建置產物的字型

Geist 字型（`@fontsource-variable/geist`）會被打包進前端建置產物。

Copyright 2024 The Geist Project Authors (https://github.com/vercel/geist-font)。

This Font Software is licensed under the SIL Open Font License, Version 1.1. 授權全文與 FAQ：https://openfontlicense.org 。完整文字也隨套件放在 `node_modules/@fontsource-variable/geist/LICENSE`。
