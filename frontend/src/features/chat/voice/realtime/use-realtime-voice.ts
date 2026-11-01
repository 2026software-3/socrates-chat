import { useCallback, useEffect, useRef, useState } from 'react'
import type { MicrophoneDiagnostics } from '@/features/chat/voice/audio/microphone'
import {
  createRealtimeSession,
  type RealtimeCloseReason,
  type RealtimePhase,
  type RealtimeSession,
} from '@/features/chat/voice/realtime/realtime-session'
import type { VoiceTurn, VoiceTurns } from '@/features/chat/voice/realtime/voice-turn'

/** 對話頁提供：語音回合怎麼進對話串，以及結束後怎麼同步狀態。 */
export type RealtimeHost = {
  conversationId: string
  turns: VoiceTurns
  /** 學生這句話已由後端保存：加進對話串並開始顯示回覆。 */
  appendStudent: (text: string) => void
  /** 一輪結束：階段、建議收斂與對話是否已結束都以後端為準。 */
  onSettled: () => void
}

export type RealtimeNotice = 'realtimeLost' | 'replyFailed' | 'permission' | 'device' | 'unavailable'

/**
 * 伺服器端語音對話（S-05.5）。連不上時回傳 `failed`，由呼叫端改用瀏覽器語音；
 * 這次頁面不再嘗試，避免每次按下都等到逾時。
 */
export function useRealtimeVoice(host: RealtimeHost | undefined, interruptible: boolean, onUnreachable: () => void) {
  const [active, setActive] = useState(false)
  const [phase, setPhase] = useState<RealtimePhase>('connecting')
  const [notice, setNotice] = useState<RealtimeNotice>()
  const [failed, setFailed] = useState(false)
  const [microphone, setMicrophone] = useState<MicrophoneDiagnostics>()

  const sessionRef = useRef<RealtimeSession>(undefined)
  const turnRef = useRef<VoiceTurn>(undefined)
  const fallbackRef = useRef(onUnreachable)
  const interruptibleRef = useRef(interruptible)

  useEffect(() => {
    fallbackRef.current = onUnreachable
  })

  useEffect(() => {
    interruptibleRef.current = interruptible
    sessionRef.current?.setInterruptible(interruptible)
  }, [interruptible])

  const endTurn = useCallback(() => {
    turnRef.current?.end()
    turnRef.current = undefined
  }, [])

  const start = useCallback(() => {
    if (!host || sessionRef.current) return
    setNotice(undefined)
    setActive(true)
    sessionRef.current = createRealtimeSession(
      host.conversationId,
      {
        onPhase: setPhase,
        onMicrophone: setMicrophone,
        onTranscript(text) {
          // 上一輪的文字可能還沒收尾：新的一句開始，它就結束了
          endTurn()
          turnRef.current = host.turns.begin()
          host.appendStudent(text)
        },
        onReplyDelta: (text) => turnRef.current?.push(text),
        onReplyEnd: endTurn,
        onReplyFailed() {
          turnRef.current?.fail()
          turnRef.current = undefined
        },
        onSettled: () => host.onSettled(),
        onClosed(reason: RealtimeCloseReason) {
          sessionRef.current = undefined
          endTurn()
          setActive(false)
          if (reason === 'unreachable') {
            setFailed(true)
            fallbackRef.current()
          } else {
            setNotice(reason === 'lost' ? 'realtimeLost' : reason)
            if (reason === 'lost' || reason === 'replyFailed') host.onSettled()
          }
        },
      },
      interruptibleRef.current,
    )
  }, [endTurn, host])

  const stop = useCallback(() => {
    sessionRef.current?.close()
    sessionRef.current = undefined
    endTurn()
    setActive(false)
  }, [endTurn])

  // 離開頁面或對話結束時關掉麥克風與連線
  useEffect(
    () => () => {
      sessionRef.current?.close()
      sessionRef.current = undefined
      turnRef.current?.end()
    },
    [],
  )

  return {
    available: host !== undefined && !failed,
    active,
    phase,
    notice,
    microphone,
    start,
    stop,
    interrupt: () => sessionRef.current?.interrupt(),
    dismissNotice: () => setNotice(undefined),
  }
}
