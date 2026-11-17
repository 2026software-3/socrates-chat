import { lazyPage } from '@/app/lazy-page'

// 對話頁帶著 assistant-ui 與語音模組，只有學生會用到：拆成獨立 chunk，教師與管理者不必下載
export const ConversationPage = lazyPage(() => import('@/features/chat/conversation-page'), 'ConversationPage')
