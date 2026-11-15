"""smoke 測試用的假內部 LLM：取代 app 的 /internal/cascade/v1，回固定的串流文字。

只驗證 cascade 鏈路（ASR → LLM → TTS），不含真正的 Rust 後端。權杖不對就回 401，和真的一樣。
"""
import json
import os

import uvicorn
from fastapi import FastAPI, Header, HTTPException
from fastapi.responses import StreamingResponse

TOKEN = os.environ['CASCADE_LLM_TOKEN']
REPLY = ['這是個很有意思的想法。', '你為什麼會這樣認為呢？']
app = FastAPI()
seen = []


@app.get('/internal/cascade/v1/models')
def models():
    return {'object': 'list', 'data': [{'id': 'socrates', 'object': 'model'}]}


@app.post('/internal/cascade/v1/chat/completions')
async def chat(body: dict, authorization: str = Header('')):
    if authorization != f'Bearer {TOKEN}':
        raise HTTPException(status_code=401)
    # 只記學生這句話的長度，不記內容
    seen.append(len(json.dumps(body.get('messages', []), ensure_ascii=False)))

    def events():
        for piece in REPLY:
            yield 'data: ' + json.dumps({'choices': [{'delta': {'content': piece}, 'finish_reason': None}]}) + '\n\n'
        yield 'data: {"choices":[{"delta":{},"finish_reason":"stop"}]}\n\ndata: [DONE]\n\n'

    return StreamingResponse(events(), media_type='text/event-stream')


if __name__ == '__main__':
    uvicorn.run(app, host='0.0.0.0', port=3000, access_log=False)
