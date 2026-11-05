# cascade 設定（docker 版）。欄位說明見 docs/specs/cascade-voice-spike.md 的「cascade 設定」。
# ${...} 由 entrypoint.sh 在啟動時代入；密碼不寫在檔案裡，LLM 節點以 api_key_env 讀 CASCADE_LLM_TOKEN。
config_cls: StreamingCascadePipelineConfig
model_path: /app

stages:
  vad:
    factory:
      silence_duration_ms: 1000

session_nodes:
  vad:
    stages: [vad]
    inputs: [input]
  turn_detection:
    stages: [turn_detection]
    inputs: ["input:pcm16", "input:session", "vad:session"]
  asr:
    kind: openai_transcription
    inputs: [input, "turn_detection:session"]
    base_url: "http://cascade-adapter:8103/v1"
    model: whisper-small
    language: zh
    response_format: json
  dialog:
    kind: dialog
    inputs:
      - asr
      - {source: "llm:text_delta", feedback: true, overflow: drop_oldest, capacity: 1024}
      - {source: "tts:session", feedback: true, overflow: drop_oldest, capacity: 1024}
      - "turn_detection:session"
      - "input:session"
      - {kind: control, source: input, event: interrupt}
      - {kind: control, source: aligner, event: reply.progress}
  llm:
    kind: openai_chat
    inputs:
      - dialog
      - "input:session"
      - {kind: control, source: dialog, event: interrupt}
    base_url: "http://app:3000/internal/cascade/v1"
    model: socrates
    api_key_env: CASCADE_LLM_TOKEN
    max_tokens: 512
  tts:
    kind: openai_speech
    inputs:
      - llm
      - "input:session"
      - {kind: control, source: dialog, event: interrupt}
    base_url: "http://cascade-adapter:8103/v1"
    model: Qwen/Qwen3-TTS-12Hz-0.6B-Base
    reference_audio_path: /refs/reference.wav
    reference_text: "${CASCADE_REFERENCE_TEXT}"
    language: Chinese
  aligner:
    kind: alignment
    method: sentence
    inputs: ["llm:text_delta", "tts:pcm_f32le", "tts:session", "dialog:session", "input:session"]

session_publish:
  turn_detection: [session]
  asr: [transcript]
  dialog: [session]
  llm: [text_delta]
  tts: [pcm_f32le, session]
  aligner: [session]
