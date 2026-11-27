"""量測客戶端。SCEN=turn|pause|interrupt，ROUNDS 輪，每輪新開一個 realtime 場次。
學生聲音用 adapter 的 TTS 合成一次後重複使用；延遲都以「最後一個語音幀送出」為 0。只印時間與計數，不印辨識全文。"""
import asyncio, base64, json, os, sys, time, uuid
import httpx, numpy as np, websockets

URL = os.environ['CASCADE_URL']; ADAPTER = os.environ['ADAPTER_URL']
SCEN = os.environ.get('SCEN', 'turn'); ROUNDS = int(os.environ.get('ROUNDS', '5')); TAG = os.environ.get('TAG', 'c')
FR = 1600
S_FULL = '我覺得民主制度比較能保障個人自由，因為人民可以自己選擇領導者。'
S_A = '我覺得民主制度比較能保障個人自由，'
S_B = '因為人民可以自己選擇領導者。'
S_INT = '等一下，我想補充一個想法。'

def synth(text):
    r = httpx.post(ADAPTER, json={'input': text, 'response_format': 'pcm', 'model': 'x', 'voice': 'x'}, timeout=90)
    r.raise_for_status()
    p = np.frombuffer(r.content, dtype='<i2').astype(np.float32) / 32768
    return p[(np.arange(int(len(p) * 16000 / 24000)) * 1.5).astype(int)]

def trim(x, th=0.01):
    idx = np.where(np.abs(x) > th)[0]
    return x[idx[0]:idx[-1] + 1] if len(idx) else x

def sil(s): return np.zeros(int(16000 * s), dtype=np.float32)
def enc(x): return base64.b64encode((np.clip(x, -1, 1) * 32767).astype('<i2').tobytes()).decode()
def ev(p): return json.dumps({**p, 'event_id': uuid.uuid4().hex})

async def one_round(main_audio, main_speech_end_s, int_audio):
    t = {}; counts = {'speech_started': 0, 'speech_stopped': 0, 'committed': 0, 'transcripts': 0, 'done': 0, 'audio_deltas': 0}
    times = {'started': [], 'stopped': [], 'transcript': [], 'first_audio': [], 'audio': [], 'done': []}
    state = {'phase': 'main', 'idx': 0, 'int_idx': 0, 'trigger_at': None, 'int_send': None}
    async with websockets.connect(URL, max_size=None) as ws:
        await ws.send(ev({'type': 'session.update', 'session': {'type': 'realtime', 'instructions': 'measure',
                    'audio': {'input': {'format': {'type': 'audio/pcm', 'rate': 16000}}}}}))
        stop = asyncio.Event()

        async def sender():
            t0 = time.time(); seq = 0
            while not stop.is_set():
                now = time.time()
                if state['phase'] == 'main':
                    s = main_audio[state['idx']:state['idx'] + FR]
                    state['idx'] += FR
                    if 'speech_end' not in t and state['idx'] / 16000 >= main_speech_end_s:
                        t['speech_end'] = time.time()
                    if state['idx'] >= len(main_audio):
                        state['phase'] = 'idle'
                elif state['phase'] == 'idle':
                    s = sil(0.1)
                    if int_audio is not None and state['trigger_at'] and now >= state['trigger_at']:
                        state['phase'] = 'int'; state['int_send'] = now
                else:
                    s = int_audio[state['int_idx']:state['int_idx'] + FR]
                    state['int_idx'] += FR
                    if 'int_end' not in t and state['int_idx'] >= len(int_audio):
                        t['int_end'] = time.time()
                    if state['int_idx'] >= len(int_audio):
                        state['phase'] = 'idle2'
                if state['phase'] == 'idle2':
                    s = sil(0.1)
                if len(s) < FR: s = np.concatenate([s, sil((FR - len(s)) / 16000)])
                await ws.send(ev({'type': 'input_audio_buffer.append', 'audio': enc(s), 'sglang': {'seq': seq}}))
                seq += 1
                await asyncio.sleep(max(0, t0 + seq * FR / 16000 - time.time()))

        st = asyncio.create_task(sender())
        try:
            deadline = time.time() + 70
            while time.time() < deadline:
                try:
                    raw = await asyncio.wait_for(ws.recv(), 1.0)
                except TimeoutError:
                    raw = None
                now = time.time()
                # 結束條件：拿到要的數字就停，不等整段回覆講完
                fa = times['first_audio'][0] if times['first_audio'] else None
                if SCEN == 'turn' and fa: break
                if SCEN == 'pause' and fa and now >= fa + 4.0: break
                if SCEN == 'interrupt' and state['int_send'] and now >= state['int_send'] + 7.0: break
                if raw is None: continue
                e = json.loads(raw); ty = e.get('type', '')
                if ty == 'input_audio_buffer.speech_started': counts['speech_started'] += 1; times['started'].append(now)
                elif ty == 'input_audio_buffer.speech_stopped': counts['speech_stopped'] += 1; times['stopped'].append(now)
                elif ty == 'input_audio_buffer.committed': counts['committed'] += 1
                elif ty == 'conversation.item.input_audio_transcription.completed': counts['transcripts'] += 1; times['transcript'].append(now)
                elif ty == 'response.output_audio.delta':
                    counts['audio_deltas'] += 1; times['audio'].append(now)
                    if not times['first_audio']:
                        times['first_audio'].append(now)
                        if int_audio is not None: state['trigger_at'] = now + 0.8
                elif ty == 'response.done': counts['done'] += 1; times['done'].append(now)
                elif ty == 'error': counts['error'] = e.get('error', {}).get('code')
            else:
                counts['timeout'] = True
        finally:
            stop.set(); st.cancel()
    se = t.get('speech_end', 0)
    r = {'scen': SCEN, 'tag': TAG, **counts}
    rel = lambda x: round(x - se, 3)
    if SCEN in ('turn', 'pause'):
        r['stopped_s'] = rel(times['stopped'][-1]) if times['stopped'] else None
        r['transcript_s'] = rel(times['transcript'][0]) if times['transcript'] else None
        r['first_audio_s'] = rel(times['first_audio'][0]) if times['first_audio'] else None
        r['done_s'] = rel(times['done'][0]) if times['done'] else None
    else:
        si = state['int_send']
        after = [x for x in times['started'] if si and x >= si]
        d = [x for x in times['done'] if si and x >= si]
        r['int_started_s'] = round(after[0] - si, 3) if after else None            # 打斷說話開始 → 後端 speech_started
        r['cancel_after_started_s'] = round(d[0] - after[0], 3) if (after and d) else None   # speech_started → 第一個回覆被取消（response.done）
        if after and d:
            leftover = [x for x in times['audio'] if after[0] <= x <= d[0]]
            r['leftover_audio_chunks'] = len(leftover)                              # 取消前還送出幾包音訊
    return r

async def main():
    if SCEN == 'turn':
        sp = trim(synth(S_FULL)); main_audio = np.concatenate([sil(0.5), sp, sil(6)]); end = (0.5 * 16000 + len(sp)) / 16000; ia = None
    elif SCEN == 'pause':
        a, b = trim(synth(S_A)), trim(synth(S_B)); pz = float(os.environ.get('PAUSE', '0.8'))
        main_audio = np.concatenate([sil(0.5), a, sil(pz), b, sil(6)]); end = (0.5 * 16000 + len(a) + pz * 16000 + len(b)) / 16000; ia = None
    else:
        sp = trim(synth(S_FULL)); ia = np.concatenate([trim(synth(S_INT)), sil(5)])
        main_audio = np.concatenate([sil(0.5), sp, sil(1)]); end = (0.5 * 16000 + len(sp)) / 16000
    await asyncio.sleep(float(os.environ.get('OFFSET', '0')))
    for i in range(ROUNDS):
        try:
            print('RES ' + json.dumps(await one_round(main_audio, end, ia), ensure_ascii=False), flush=True)
        except Exception as e:
            print('RES ' + json.dumps({'scen': SCEN, 'tag': TAG, 'exception': type(e).__name__}), flush=True)
        await asyncio.sleep(1.5)

asyncio.run(main())
