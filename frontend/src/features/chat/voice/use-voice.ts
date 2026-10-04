import { useCallback, useEffect, useRef, useState } from 'react'
import {
  COUNTDOWN_MS,
  HINT_AFTER_MS,
  MAX_UTTERANCE_MS,
  SILENCE_MS,
  loadAutoSend,
  saveAutoSend,
  speechLang,
} from '@/features/chat/voice/config'
import {
  createRecognizer,
  pickRecognitionSource,
  type RecognitionSource,
  type Recognizer,
} from '@/features/chat/voice/recognizer'
import { createSpeaker, type Speaker } from '@/features/chat/voice/speaker'

/** 輸入框的存取：由對話串元件提供，hook 本身不依賴 assistant-ui。 */
export type VoiceHost = {
  /** 這場對話的語言（S-12.1）；辨識與合成語言跟隨它 */
  language: string
  getText: () => string
  setText: (text: string) => void
  send: () => void
  /** 目前這段文字是以哪種語音來源輸入的（送出時一起保存，S-05.4） */
  onSource: (source: RecognitionSource) => void
}

export type VoiceMode = 'off' | 'dictate' | 'converse'
/**
 * - `idle`：沒有在收音或播放
 * - `listening`：收音中
 * - `countdown`：靜音後的「即將送出」倒數
 * - `waiting`：已送出，等 AI 回覆
 * - `transcribing`：備援辨識已停止錄音，等後端回傳文字
 * - `review`：備援辨識的文字已填入輸入框，等學生確認（倒數或按鈕送出，或再說一段）
 * - `speaking`：播放 AI 語音
 */
export type VoicePhase = 'idle' | 'listening' | 'countdown' | 'review' | 'transcribing' | 'waiting' | 'speaking'
export type VoiceNotice = 'permission' | 'unavailable' | 'failed' | 'speechFailed' | 'hint'

export type VoiceReply = { id: string; text: string }

/**
 * 語音輸入與輪流語音對話（S-05.1、S-05.2）。
 *
 * - `dictate`：辨識文字即時填入輸入框，學生確認或編輯後自己送出（S-05.1）。
 * - `converse`：AI 回覆自動播放；播完自動收音；靜音 3 秒進入 3 秒倒數後自動送出
 *   （可關閉自動送出，只用按鈕）。播放時不收音，避免把 AI 的聲音錄成學生發言。
 * 備援的 OpenAI 辨識沒有即時文字也沒有靜音偵測，需按按鈕結束發言。
 */
export function useVoice(host: VoiceHost, reply: VoiceReply | undefined) {
  const [mode, setModeState] = useState<VoiceMode>('off')
  const [phase, setPhaseState] = useState<VoicePhase>('idle')
  const [notice, setNotice] = useState<VoiceNotice>()
  const [countdown, setCountdown] = useState(0) // 0..1
  const [autoSend, setAutoSendState] = useState(loadAutoSend)

  const hostRef = useRef(host)
  const modeRef = useRef<VoiceMode>('off')
  const phaseRef = useRef<VoicePhase>('idle')
  const autoSendRef = useRef(autoSend)
  const recRef = useRef<Recognizer>(undefined)
  const speakerRef = useRef<Speaker>(undefined)
  const baseRef = useRef('')
  const sourceRef = useRef<RecognitionSource>('speech-browser')
  const startRef = useRef<(m: Exclude<VoiceMode, 'off'>) => void>(() => {})
  const spokenRef = useRef('') // 這次發言辨識到的文字
  const lastReplyRef = useRef<VoiceReply>(undefined)
  const handledReplyRef = useRef<string>(undefined)
  const timers = useRef<{ silence?: number; countdown?: number; max?: number; hint?: number }>({})

  useEffect(() => {
    hostRef.current = host
  })

  const setPhase = useCallback((p: VoicePhase) => {
    phaseRef.current = p
    setPhaseState(p)
  }, [])
  const setMode = useCallback((m: VoiceMode) => {
    modeRef.current = m
    setModeState(m)
  }, [])

  const clearTimers = useCallback(() => {
    const t = timers.current
    window.clearTimeout(t.silence)
    window.clearInterval(t.countdown)
    window.clearTimeout(t.max)
    window.clearTimeout(t.hint)
    timers.current = {}
    setCountdown(0)
  }, [])

  const ensureSpeaker = useCallback((): Speaker => {
    const lang = hostRef.current.language
    speakerRef.current ??= createSpeaker(speechLang(lang), lang)
    return speakerRef.current
  }, [])

  /** 停止收音並丟掉辨識器（不處理剩下的結果）。 */
  const stopRecognizer = useCallback(() => {
    const rec = recRef.current
    recRef.current = undefined
    clearTimers()
    rec?.stop()
  }, [clearTimers])

  const submit = useCallback(() => {
    if (!['listening', 'countdown', 'review'].includes(phaseRef.current)) return
    const text = spokenRef.current.trim()
    if (!text) {
      // 什麼都沒說：不送出空訊息，繼續聆聽
      window.clearInterval(timers.current.countdown)
      setCountdown(0)
      setPhase(sourceRef.current === 'speech-openai' ? 'review' : 'listening')
      return
    }
    stopRecognizer()
    setPhase('waiting')
    // 等輸入框的文字更新完再送出
    window.setTimeout(() => hostRef.current.send(), 0)
  }, [setPhase, stopRecognizer])

  const startCountdown = useCallback(() => {
    if (!['listening', 'review'].includes(phaseRef.current) || !autoSendRef.current) return
    setPhase('countdown')
    const started = Date.now()
    timers.current.countdown = window.setInterval(() => {
      const p = Math.min(1, (Date.now() - started) / COUNTDOWN_MS)
      setCountdown(p)
      if (p >= 1) {
        window.clearInterval(timers.current.countdown)
        submit()
      }
    }, 100)
  }, [setPhase, submit])

  const armSilence = useCallback(() => {
    window.clearTimeout(timers.current.silence)
    if (modeRef.current !== 'converse' || !autoSendRef.current) return
    timers.current.silence = window.setTimeout(startCountdown, SILENCE_MS)
  }, [startCountdown])

  /** 倒數中又開口，或按「我還沒說完」：取消倒數、繼續聆聽，接在同一段發言。 */
  const keepListening = useCallback(() => {
    if (phaseRef.current !== 'countdown' && phaseRef.current !== 'review') return
    window.clearInterval(timers.current.countdown)
    setCountdown(0)
    if (sourceRef.current === 'speech-openai') {
      // 備援沒有持續收音：再錄一段，接在輸入框已有的文字後面
      startRef.current('converse')
      return
    }
    setPhase('listening')
    armSilence()
  }, [armSilence, setPhase])

  const startListening = useCallback(
    (m: Exclude<VoiceMode, 'off'>) => {
      stopRecognizer()
      const lang = hostRef.current.language
      const existing = hostRef.current.getText().trim()
      baseRef.current = existing ? `${existing} ` : ''
      spokenRef.current = ''
      setNotice(undefined)
      const rec: Recognizer = createRecognizer(pickRecognitionSource(), speechLang(lang), lang, {
        onText: (text, final) => {
          const first = spokenRef.current === ''
          spokenRef.current = text
          hostRef.current.setText(baseRef.current + text)
          hostRef.current.onSource(rec.source)
          if (phaseRef.current === 'countdown') keepListening()
          // 單次發言上限 60 秒：從開口才開始算（開口前不計時）
          if (first && recRef.current === rec) {
            timers.current.max = window.setTimeout(() => {
              if (modeRef.current === 'converse') submit()
              else {
                stopRecognizer()
                setPhase('idle')
              }
            }, MAX_UTTERANCE_MS)
          }
          if (modeRef.current === 'converse') {
            window.clearTimeout(timers.current.hint)
            if (rec.source === 'speech-openai') {
              // 備援一次給完整文字：直接進入倒數，學生仍可在倒數中取消或改字
              if (final) {
                setPhase('review')
                startCountdown()
              }
            } else {
              armSilence()
            }
          }
        },
        onEnd: () => {
          if (recRef.current === rec) recRef.current = undefined
          // 備援辨識結束（有文字時 onText 已先處理）；沒有收到文字就回到閒置
          if (phaseRef.current === 'transcribing') setPhase('idle')
          else if (modeRef.current === 'dictate' && phaseRef.current === 'listening') setPhase('idle')
        },
        onError: (kind) => {
          if (kind === 'fallback') {
            // 瀏覽器辨識不可用：改用 OpenAI 備援重新開始（pickRecognitionSource 之後會選到備援）
            window.setTimeout(() => startRef.current(m), 0)
            return
          }
          stopRecognizer()
          setPhase('idle')
          setNotice(kind)
          // 無法收音就不要停在語音對話模式
          if (kind === 'permission' || kind === 'unavailable') setMode('off')
        },
      })
      recRef.current = rec
      sourceRef.current = rec.source
      setPhase('listening')
      setMode(m)
      timers.current.hint = window.setTimeout(() => setNotice('hint'), HINT_AFTER_MS)
      rec.start()
    },
    [armSilence, keepListening, setMode, setPhase, startCountdown, stopRecognizer, submit],
  )

  useEffect(() => {
    startRef.current = startListening
  }, [startListening])

  /** 播放 AI 回覆；播完（或被停止）後輪到學生。 */
  const speakReply = useCallback(
    async (r: VoiceReply, thenListen: boolean) => {
      stopRecognizer()
      setPhase('speaking')
      let failed = false
      try {
        await ensureSpeaker().speak(r.text)
      } catch {
        failed = true
        setNotice('speechFailed')
      }
      // 播放期間若被關掉語音對話，就不再收音
      if (modeRef.current !== 'converse') {
        setPhase('idle')
        return
      }
      if (thenListen || failed) startListening('converse')
      else setPhase('idle')
    },
    [ensureSpeaker, setPhase, startListening, stopRecognizer],
  )

  // AI 回覆完成：在語音對話中自動播放
  useEffect(() => {
    lastReplyRef.current = reply
    if (!reply || handledReplyRef.current === reply.id) return
    if (modeRef.current === 'converse' && phaseRef.current === 'waiting') {
      handledReplyRef.current = reply.id
      void speakReply(reply, true)
    }
  }, [reply, speakReply])

  const stopAll = useCallback(() => {
    stopRecognizer()
    speakerRef.current?.stop()
    setMode('off')
    setPhase('idle')
    setNotice(undefined)
  }, [setMode, setPhase, stopRecognizer])

  // 離開頁面或對話結束時關掉收音與播放
  useEffect(
    () => () => {
      recRef.current?.stop()
      speakerRef.current?.stop()
      const t = timers.current
      window.clearTimeout(t.silence)
      window.clearInterval(t.countdown)
      window.clearTimeout(t.max)
      window.clearTimeout(t.hint)
    },
    [],
  )

  return {
    mode,
    phase,
    notice,
    countdown,
    autoSend,
    /** 語音輸入：開始收音（再按一次停止）；播放中按下會立即停止播放並開始收音（S-05.2 打斷）。 */
    toggleDictate() {
      if (mode === 'dictate' && phase === 'listening') {
        if (rec_isOpenAi(recRef.current)) {
          // 備援：先結束錄音，等後端辨識完把文字填入輸入框
          setPhase('transcribing')
          clearTimers()
          recRef.current?.stop()
          return
        }
        stopRecognizer()
        setPhase('idle')
        return
      }
      if (mode === 'converse') {
        // 語音對話中按「說話」：打斷播放並開始收音
        speakerRef.current?.stop()
        if (phaseRef.current !== 'speaking') startListening('converse')
        return
      }
      startListening('dictate')
    },
    toggleConverse() {
      if (mode === 'converse') {
        stopAll()
        return
      }
      stopRecognizer()
      setMode('converse')
      const last = lastReplyRef.current
      // 一開始先聆聽；之後每個 AI 回覆自動播放
      handledReplyRef.current = last?.id
      startListening('converse')
    },
    /** 立即送出（倒數中或收音中）。 */
    sendNow() {
      if (rec_isOpenAi(recRef.current) && phaseRef.current === 'listening') {
        // 備援：先結束錄音取得文字，辨識完會進入倒數
        setPhase('transcribing')
        clearTimers()
        recRef.current?.stop()
        return
      }
      submit()
    },
    keepListening,
    /** 停止播放，直接輪到學生。 */
    stopSpeaking() {
      speakerRef.current?.stop()
    },
    replay() {
      const r = lastReplyRef.current
      if (r) void speakReply(r, true)
    },
    setAutoSend(on: boolean) {
      autoSendRef.current = on
      setAutoSendState(on)
      saveAutoSend(on)
      if (!on) {
        window.clearTimeout(timers.current.silence)
        if (phaseRef.current === 'countdown') keepListening()
      } else if (phaseRef.current === 'listening') armSilence()
    },
    dismissNotice: () => setNotice(undefined),
  }
}

function rec_isOpenAi(rec: Recognizer | undefined): boolean {
  return rec?.source === 'speech-openai'
}
