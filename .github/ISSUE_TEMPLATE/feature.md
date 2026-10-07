---
name: 功能
about: 功能父 Issue（F-xx）、功能切片（F-xx.n）或面向任務（F-xx.n-DB/AI/BE/SEC/FE）
title: "[F-xx.n] "
---

<!--
Issue Type（建立後手動選擇）：功能父 Issue 與再往下拆的核心功能切片選 Feature；實際被指派的葉節點（面向任務、其他功能切片）選 Task。
Labels：
- 功能父 Issue，以及再往下拆的核心功能切片：不加 label。
- 面向任務：只標 1 個面向（frontend / backend / ai / database / security）。
- 其他功能切片（葉節點）：標實際涉及的面向。
層級：用 GitHub 的 sub-issue 功能掛在上一層底下；相依用 blocked-by 設定。前端選型（S-11）已定案，帶 `frontend` 的任務不再因此被擋住。
規則細節不要寫在這裡，請連到 `docs/specs/`。
-->

## 使用者情境／目標

身為【角色】，我想要【目標】，以便【效益】。

## Given / When / Then

- Given ，When ，Then 

## 權限、資料與失敗情境

-

## 規格依據

<!-- 列出相關規則文件的連結，例如 docs/specs/decisions.md、docs/specs/technical-details.md。尚未決定的規格請連到該規格 Issue。 -->

-

## 先行測試（TDD）

- [ ] 先新增會失敗的測試，覆蓋主要成功流程
- [ ] 覆蓋權限、錯誤或邊界情境
- [ ] AI 行為使用固定情境與行為評分準則，不要求輸出逐字相同（如適用）
- [ ] 以合成資料驗證授權與個資條件（如適用）

## 最小改動範圍

- 本 Issue 要改的功能：
- 明確不包含：

## 相依

<!-- 請用 GitHub 的 blocked-by 功能設定；這裡可簡述原因。 -->

-

## 完成條件

- [ ] 規格、測試與實作一致，新增測試全部通過
- [ ] 權限與個資條件有測試證據
- [ ] 若改動涉及規則，已同步更新 `docs/specs/`
