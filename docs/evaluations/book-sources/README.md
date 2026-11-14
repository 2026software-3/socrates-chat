# S-06.3 書目來源驗證總結

測試日期：2026-10-30。對應 #130；已確認 Google Books API 優先，回傳零筆或候選查核後全部不相關／無法確認書目時使用 Open Library；保存相容性仍待確認。本頁彙整選型證據；精簡查詢結果與固定樣本隨附，完整 API 紀錄保留於本機備份。

## 已做的驗證

| 項目 | 範圍 | 方法 |
| --- | --- | --- |
| 已知書目搜尋 | 中文 20 筆、英文 8 筆、西文 8 筆 | 兩來源各查書名、書名＋作者、有效 ISBN；搜尋取前五筆 |
| 概念搜尋 | 分配正義、個人自主、道德責任，各三語 | 兩來源比較有／無語言篩選，共 36 次查詢 |
| 候選相關性 | 語言篩選組 75 個候選位置 | 全部題名初審，8 部作品另核對出版社內容，對應 11 個位置 |
| 版本與缺值 | 作者、ISBN、年份、語言、作品關係 | 核對來源回應及出版社／館藏資料 |
| 條款與容量 | 保存、署名、移除、請求速率 | 查官方文件；60 人情境僅作算式估算 |

共 250 次書目／概念 API 請求，兩來源各 125 次，不含先前無金鑰 429 探測。順序請求、間隔至少 1.1 秒、10 秒逾時，不自動重試。未呼叫 LLM、未使用學生資料、未做負載測試。

## 結果

以下為任一查法出現目標 ISBN，**不是完整書目正確率或推薦成功率**。

| 樣本 | Google Books | Open Library |
| --- | ---: | ---: |
| 博客來中文組：8 個不重複有效 ISBN | 6/8 | 1/8 |
| 哲學新媒體中文組：10 本 | 5/10＊ | 0/10 |
| 英文組：8 本 | 7/8 | 8/8 |
| 西文組：8 本 | 5/8 | 5/8 |

＊Google 一筆 ISBN 相符但書名衝突，仍待確認。西文兩來源找到的書不同，合併可涵蓋 7/8。

- **中文**：Google 在本次樣本較有覆蓋；Open Library 三組中文概念查詢均無結果。
- **相關性**：深查的 11 個候選位置中，9 個相關、2 個偏離本輪哲學情境；另外 64 個內容證據不足，不能當作不相關。深查為有目的抽樣，不計供應商通過率。
- **資料品質**：有作者遺漏、版本語言缺值、同名不同書及作品關係不完整。Google 中文篩選仍回傳標為日／韓文的候選；不能只靠查詢參數判定語言。
- **可用性**：有金鑰後的低頻測試未遇限流／逾時；不代表 60 人同時使用可通過。
- **保存相容性**：Google 一般 API 條款對永久副本及快取有限制，尚未確認本案持久匯入／歷史快照的許可或例外。Open Library 有目錄再利用依據，但不能把簡介、封面一律當成無限制素材。官方依據見下方「保存查核」。

樣本非隨機，中文組與英西文組的選樣不同；博客來取搜尋索引、未核實即時排名。不能用本表推估全書庫或各語言整體覆蓋率。

## 候選查核

[candidates.csv](candidates.csv) 保留 75 個位置的來源 ID、版本資料、評級、理由及出版社連結。合成情境為資源分配、自主選擇與道德歸責，於搜尋後補作審閱用途。題名初篩與內容評級分開；由 AI 輔助審閱，未經教師複評、未讀全文。相關表示出版社簡介／目錄支持核心問題；資訊不足不等於不相關。

代表案例：

- 《自由主義的道德處境》ISBN、作者與出版日期符合聯經資料，但 Google 語言標為 `zh-CN`；不能據此直接判定繁簡。
- *Debates sobre justicia distributiva* 的 Google 紀錄缺一位編者；ISBN 命中不保證作者完整。重印年份也不能覆寫原版本年份。
- *Autonomía personal y salud infantil* 書目與出版社相符，但內容為幼兒自理訓練，不適合本輪成人自主的哲學情境。
- Open Library 搜尋回傳 work 彙整資料；ISBN、語言集合不能任意配成同一版本。不同 work ID 也不能證明是不同作品。

## 保存查核

文件核對至 2026-11-09，未取得額外授權；以下為尚未完成的相容性查核，不表示所有書目事實都禁止保存。

| 官方依據 | 本案需確認的事項 |
| --- | --- |
| [Google API 條款 §5、§8](https://developers.google.com/terms)、[Books 條款](https://developers.google.com/books/terms) | 共用書庫與歷史快照中的書名、作者、ISBN、語言、出版年、來源 ID／連結及選填簡介，適用的長期保存依據與移除要求尚未確認；原測試未保存 cache headers |
| [Google 品牌指引](https://developers.google.com/books/branding) | Google 內容署名、連結與修改限制；「教師書單」標示不等於資料提供者署名 |
| [Open Library 授權](https://openlibrary.org/developers/licensing)、[使用 FAQ](https://openlibrary.org/help/faq/using) | 目錄再利用有官方依據，但簡介與封面不能一律視為無限制素材 |
| [Google 使用文件](https://developers.google.com/books/docs/v1/using)、[Open Library API 指引](https://openlibrary.org/developers/api) | Google 專案實際額度未確認；Open Library 預設 1 request/s，識別應用為 3 requests/s；尚未驗證課堂併發容量 |

### 與既有規格的對照

| 既有需求 | 本輪查核結論 |
| --- | --- |
| 第 5 節：外部搜尋結果可確認／修正後持久匯入書庫 | Google API §5(e)(1) 限制永久副本與資料庫，另有權利人或適用法律允許的例外；目前未找到適用本案的明確依據。教師點選確認本身不證明符合例外 |
| 第 7、8 節：保存當次推薦書目，日後不隨書庫更新 | 同樣涉及外部書目長期保存；只存最終選定幾本、只讓本人看，尚不足以證明符合上述例外 |
| 第 5 節：教師可修正預填資料 | Google 品牌指引另要求不得變更 API 結果／內容；須釐清外部原始資料與教師修正資料的呈現方式，不能只加來源標籤就宣稱相容 |
| 第 8 節：既有資料生命週期 | Google API §8(b) 涉及終止後刪除依 cache header 保存的內容；Books 條款另有侵權移除要求。未找到可據以承諾永久不變的許可 |
| Open Library 基本書目匯入與快照 | 官方 FAQ 明確提供目錄下載、匯出及 CC0 貢獻政策，可作為目錄再利用依據；licensing 頁仍保留既有權利限制，不能擴張為所有簡介、封面均可使用 |

[Google Books 概覽](https://developers.google.com/books/docs/overview)亦說明其部分資料由他人授權，並非 Google 可任意再散布。Books API 可直接使用，不需事前核准；這與本案是否能長期保存是不同問題。

**結論：尚不能將「Google 優先且其回應直接長期匯入／保存快照」記為相容性通過。** 這是目前證據不足，不是判定所有基本書目事實都受著作權保護或一律禁止保存。多測 API、補抓 cache headers，均不能直接取得長期保存許可。

來源決策維持 Google 優先與既有保存需求；長期保存的適用依據尚未確認，作為本次 PR 的已知限制供審查，不代表已取得額外授權。若後續需調整來源或保存規則，仍經規格 PR。

已確認保存需求優先；若來源條件不相容，調整來源或取得方式。自行整理的書目與外部回應需分辨，不能只因教師點選匯入或 AI 改寫就視為取得保存許可。產品規則與後續工作見 [S-06.3](../../specs/S-06.3-book-recommendations.md)。

## 證據與重跑

- `*-2026-10-30.json` 與 `philomedium-2025.json`：五份固定樣本，內含選樣方法、查詢詞與資料來源。
- [requests.csv](requests.csv)：逐次查詢的樣本位置、模式、URL、狀態、耗時、候選數及目標 ISBN 命中的來源 ID／書名。ISBN-10 換算 ISBN-13 後比較；Open Library work 命中僅表示其彙整集合包含該 ISBN。
- [candidates.csv](candidates.csv)：語言篩選組的 75 個候選位置與內容查核證據。兩份 CSV 由既有紀錄摘錄，非重新測試；未保留所有非命中候選及整批 ISBN／版本清單。

從 repo 根目錄執行，輸出放到本機暫存目錄；將 `--provider` 改為 `openlibrary` 可重跑另一來源。Google 金鑰只從本機 `.env` 讀取，不會寫進結果。以下會呼叫外部 API，搜尋結果可能隨時間變動；此次整理未重跑。

```bash
python3 scripts/evaluate-book-sources.py \
  docs/evaluations/book-sources/en-es-philosophy-2026-10-30.json \
  /tmp/books-google-rerun.json --provider google --key-file .env
python3 scripts/evaluate-book-concepts.py \
  docs/evaluations/book-sources/concepts-2026-10-30.json \
  /tmp/concepts-google-rerun.json --provider google --key-file .env
```

本次僅支持來源順序及查核風險的決策，未驗證 LLM 個人化推薦、完整 fallback 流程、權限或負載；功能測試留待對應實作任務。
