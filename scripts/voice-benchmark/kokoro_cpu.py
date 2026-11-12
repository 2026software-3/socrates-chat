import os,time,resource,json,sys
os.environ.setdefault('HF_HOME', '/tmp/voice-claim-hf')
import torch
from kokoro import KPipeline
lang,voice,text=sys.argv[1:]
torch.set_num_threads(4)
t0=time.perf_counter(); p=KPipeline(lang_code=lang,device='cpu'); loaded=time.perf_counter()
t1=time.perf_counter(); first=None; chunks=[]
for _,_,audio in p(text,voice=voice):
 if first is None: first=time.perf_counter()
 chunks.append(audio)
end=time.perf_counter(); samples=sum(len(x) for x in chunks)
if os.environ.get('VOICE_OUTPUT'):
 import numpy as np, soundfile as sf
 sf.write(os.environ['VOICE_OUTPUT'], np.concatenate([x.numpy() for x in chunks]), 24000)
print('BENCHMARK '+json.dumps({'lang':lang,'voice':voice,'device':'cpu','threads':4,'load_s':loaded-t0,'first_audio_s':first-t1 if first else None,'elapsed_s':end-t1,'audio_s':samples/24000,'rtf':(end-t1)/(samples/24000) if samples else None,'peak_rss_mib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss/1024},ensure_ascii=False),flush=True)
