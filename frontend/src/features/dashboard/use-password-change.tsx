import { useState } from 'react'
import { ChangePasswordDialog } from '@/features/dashboard/change-password-section'

/**
 * 個人設定卡片共用的「更改密碼」狀態：`start` 開啟視窗並清掉舊的確認訊息，`saved` 是成功後要顯示的確認，
 * `dialog` 是要渲染在卡片（與任何 `<form>`）外面的視窗。學生與教師、管理者的卡片都用它。
 */
export function usePasswordChange() {
  const [open, setOpen] = useState(false)
  const [saved, setSaved] = useState(false)
  return {
    saved,
    start: () => {
      setSaved(false)
      setOpen(true)
    },
    dialog: <ChangePasswordDialog open={open} onOpenChange={setOpen} onSaved={() => setSaved(true)} />,
  }
}
