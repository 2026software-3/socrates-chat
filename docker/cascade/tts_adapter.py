"""cascade 與 TTS 之間的轉接：繁轉簡，並依字數帶 max_new_tokens。

實測（results/capped）：繁體直送與不帶上限都會讓 0.6B 暴走；cascade 的 openai_speech
兩者都不處理，所以在這裡補上。其餘欄位與串流回應原樣轉發。
"""
import os
import time

import httpx
import uvicorn
from fastapi import FastAPI, Request
from fastapi.responses import Response, StreamingResponse
from opencc import OpenCC

UPSTREAM = os.environ.get("TTS_UPSTREAM", "http://127.0.0.1:8100/v1/audio/speech")
WHISPER_UPSTREAM = os.environ.get("WHISPER_UPSTREAM", "http://127.0.0.1:8101/v1/audio/transcriptions")
TOKENS_PER_CHAR = 6.25
MIN_TOKENS = 30

to_simple = OpenCC("t2s")
client = httpx.AsyncClient(timeout=httpx.Timeout(60))
app = FastAPI()


@app.get("/v1/models")
def models():
    """cascade 啟動時探測這個端點；whisper-server.py 沒有。"""
    return {"object": "list", "data": []}


@app.post("/v1/audio/transcriptions")
async def transcriptions(request: Request):
    """原樣轉發給 whisper，音訊不落地。"""
    res = await client.post(
        WHISPER_UPSTREAM,
        content=await request.body(),
        headers={"content-type": request.headers["content-type"]},
    )
    print("asr", {"status": res.status_code, "s": round(res.elapsed.total_seconds(), 3)}, flush=True)
    return Response(res.content, status_code=res.status_code, media_type="application/json")


@app.post("/v1/audio/speech")
async def speech(request: Request):
    body = await request.json()
    body["input"] = to_simple.convert(body["input"])
    body["max_new_tokens"] = max(MIN_TOKENS, int(len(body["input"]) * TOKENS_PER_CHAR))
    started = time.time()
    upstream = await client.send(client.build_request("POST", UPSTREAM, json=body), stream=True)
    print("tts", {"chars": len(body["input"]), "cap": body["max_new_tokens"],
                  "status": upstream.status_code, "headers_s": round(time.time() - started, 3)}, flush=True)

    async def forward():
        try:
            async for data in upstream.aiter_raw():
                yield data
        finally:
            await upstream.aclose()

    headers = {k: v for k, v in upstream.headers.items() if k.lower().startswith("x-")}
    return StreamingResponse(forward(), status_code=upstream.status_code, headers=headers,
                             media_type=upstream.headers.get("content-type"))


if __name__ == "__main__":
    uvicorn.run(app, host=os.environ.get("ADAPTER_HOST", "127.0.0.1"), port=8103, log_level="warning")
