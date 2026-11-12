"""Single-request CUDA measurement; model weights must already be cached."""
import json
import os
import resource
import subprocess
import sys
import threading
import time

import numpy as np
import soundfile as sf
import torch

model_name, language, voice, text, output = sys.argv[1:]
torch.set_num_threads(4)
if not torch.cuda.is_available():
    raise RuntimeError('CUDA is unavailable')

def device_memory_mib():
    result = subprocess.run(
        ['nvidia-smi', '--query-gpu=memory.used', '--format=csv,noheader,nounits', '-i', '0'],
        capture_output=True, text=True, check=True,
    )
    return int(result.stdout.strip())

baseline = device_memory_mib()
samples = [baseline]
stop = threading.Event()
def monitor():
    while not stop.wait(0.1):
        try:
            samples.append(device_memory_mib())
        except (OSError, subprocess.SubprocessError, ValueError):
            pass
thread = threading.Thread(target=monitor, daemon=True)
thread.start()
torch.cuda.reset_peak_memory_stats()
t0 = time.perf_counter()
if model_name == 'kokoro':
    from kokoro import KPipeline
    pipeline = KPipeline(lang_code=language, device='cuda')
else:
    from melo.api import TTS
    pipeline = TTS(language=language, device='cuda')
torch.cuda.synchronize()
loaded = time.perf_counter()
t1 = time.perf_counter()
if model_name == 'kokoro':
    chunks = []
    first = None
    for _, _, chunk in pipeline(text, voice=voice):
        torch.cuda.synchronize()
        if first is None:
            first = time.perf_counter()
        chunks.append(chunk.numpy())
    audio = np.concatenate(chunks)
    sample_rate = 24000
else:
    speaker = next(iter(pipeline.hps.data.spk2id.values()))
    audio = pipeline.tts_to_file(text, speaker, quiet=True)
    torch.cuda.synchronize()
    first = time.perf_counter()
    sample_rate = pipeline.hps.data.sampling_rate
end = time.perf_counter()
samples.append(device_memory_mib())
stop.set()
thread.join()
sf.write(output, audio, sample_rate)
duration = len(audio) / sample_rate
print('BENCHMARK ' + json.dumps({
    'model': model_name, 'language': language, 'voice': voice,
    'dtype': str(next(pipeline.model.parameters()).dtype),
    'device': torch.cuda.get_device_name(0), 'torch': torch.__version__,
    'load_s': loaded - t0, 'first_audio_s': first - t1,
    'elapsed_s': end - t1, 'audio_s': duration, 'rtf': (end - t1) / duration,
    'peak_rss_mib': resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024,
    'cuda_peak_allocated_mib': torch.cuda.max_memory_allocated() / 1048576,
    'cuda_peak_reserved_mib': torch.cuda.max_memory_reserved() / 1048576,
    'device_baseline_mib': baseline, 'device_peak_sampled_mib': max(samples),
    'device_samples': len(samples),
}), flush=True)
