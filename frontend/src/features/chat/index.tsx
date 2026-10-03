import type { Feature } from '@/app/feature'
import { ConversationListPage } from '@/features/chat/conversation-list-page'
import { ConversationPage } from '@/features/chat/conversation-page'

export const chatFeature: Feature = {
  routes: [
    { path: '/conversations', element: <ConversationListPage />, access: 'member' },
    { path: '/conversations/:id', element: <ConversationPage />, access: 'member' },
  ],
  nav: [{ to: '/conversations', labelKey: 'chat.nav.conversations', access: 'member' }],
}
