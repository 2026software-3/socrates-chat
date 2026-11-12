// 規格已確定但後端／前端尚未實作的項目。以 todo 列出，實作完成時把對應項目改成真正的測試。
// （todo 不會讓執行失敗；實作後的測試請放進對應主題的檔案。）
import { describe, test } from 'node:test';

const todo = { todo: '尚未實作' };

describe('S-07 思辨點數與任務（docs/specs/S-07-points-and-tasks.md）', () => {
  test('S-07.2 完成一場對話（已產生 AI 總結）發 10 點，不設每日上限', todo);
  test('S-07.2 同一事件重複送出只入帳一次（UNIQUE (user_id, source_type, source_id)），重複視為成功', todo);
  test('S-07.2 餘額不得為負；扣除與餘額檢查在同一交易內', todo);
  test('S-07.2 學生刪除已得點的對話不追回點數；帳號刪除時帳本一併刪除', todo);
  test('S-07.4 每日／每週任務（Asia/Taipei、週一起算）達成後手動領取，同週期只能領一次，逾期失效', todo);
  test('S-07.4 角色部件解鎖永久、不可退款；調價與停用不影響已擁有者', todo);
  test('S-07.4 只有系統管理者能修改部件價格、啟用狀態與任務數值，並寫入稽核日誌', todo);
});

describe('S-12.1 多語言', () => {
  test('首次登入依 Accept-Language 設定語言（es-MX → es；fr → zh-TW），之後以使用者設定為準', todo);
  test('使用者可切換語言設定並立即生效；新對話複製語言設定，進行中的對話語言不變', todo);
  test('AI 提示與規則版本含語言（例如 v1/en），AI 與總結以對話語言產生', todo);
  test('API 錯誤只回傳穩定錯誤代碼與 request ID，不含已翻譯文字', todo);
});

describe('S-02.3 個人雷達圖公開規則', () => {
  test('預設不公開；公開後課程內所有人可見，只顯示角色外觀與顯示名稱', todo);
});

describe('S-06 閱讀推薦與資料查詢', () => {
  test('S-06.4 只顯示後端查到的來源 ID，模型編造的書目一律刪除', todo);
  test('S-06.4 外部資料庫失敗或逾時（10 秒）時只顯示教師書單並註明，不影響主對話', todo);
});

describe('S-09.3 維運端點', () => {
  test('/ready 檢查資料庫連線', todo);
});

describe('S-01.3 管理者保護', () => {
  test('系統至少保留 1 位啟用中的管理者（目前只有「不能停用自己」可由 API 觸發）', todo);
});

describe('S-10 效能與瀏覽器支援', () => {
  test('S-10.2 一般 API p95 ≤ 500ms（60 人同時，需獨立的負載測試工具）', todo);
  test('S-10.1 瀏覽器相容性（需瀏覽器自動化，屬 UI e2e，尚未導入）', todo);
});
