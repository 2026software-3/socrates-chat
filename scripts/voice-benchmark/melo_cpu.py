"""Single-request CPU measurement for MeloTTS; model weights must already be cached."""
import json
import os
import resource
import sys
import time

os.environ.setdefault('HF_HOME', '/tmp/voice-claim-melo-hf')
import torch
from melo.api import TTS

lang, text = sys.argv[1:]
torch.set_num_threads(4)
t0 = time.perf_counter()
model = TTS(language=lang, device='cpu')
loaded = time.perf_counter()
speaker = next(iter(model.hps.data.spk2id.values()))
t1 = time.perf_counter()
audio = model.tts_to_file(text, speaker, quiet=True)
end = time.perf_counter()
duration = len(audio) / model.hps.data.sampling_rate
if os.environ.get('VOICE_OUTPUT'):
    import soundfile as sf

    sf.write(os.environ['VOICE_OUTPUT'], audio, model.hps.data.sampling_rate)
print('BENCHMARK ' + json.dumps({
    'lang': lang, 'device': 'cpu', 'threads': 4,
    'load_s': loaded - t0,
    'first_audio_s': end - t1,  # MeloTTS 回傳整段音訊，不是串流 TTFA
    'elapsed_s': end - t1,
    'audio_s': duration,
    'rtf': (end - t1) / duration,
    'peak_rss_mib': resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024,
}, ensure_ascii=False), flush=True)
