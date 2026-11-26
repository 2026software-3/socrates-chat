"""測試用假內部 LLM：回較長的串流文字（讓打斷有東西可切），權杖不對回 401。環境變數 CASCADE_LLM_TOKEN、STUB_TAG。"""
import asyncio, json, os
import uvicorn
from fastapi import FastAPI, Header, HTTPException
from fastapi.responses import StreamingResponse

TOKEN = os.environ['CASCADE_LLM_TOKEN']
TAG = os.environ.get('STUB_TAG', 'a')
REPLY = ['這是個很有意思的想法。', '你為什麼會這樣認為呢？', '我們可以先想想，自由的意思到底是什麼。',
         '假如有人選了你不同意的領導者，你覺得這還算自由嗎？', '請試著舉一個生活中的例子。', '再說說看你的理由。']
app = FastAPI()
hits = {'n': 0}

@app.get('/internal/cascade/v1/models')
def models():
    return {'object': 'list', 'data': [{'id': 'socrates', 'object': 'model'}]}

@app.get('/hits')
def get_hits():
    return {'tag': TAG, 'chat_calls': hits['n']}

@app.post('/internal/cascade/v1/chat/completions')
async def chat(body: dict, authorization: str = Header('')):
    if authorization != f'Bearer {TOKEN}':
        raise HTTPException(status_code=401)
    hits['n'] += 1
    async def events():
        for piece in REPLY:
            yield 'data: ' + json.dumps({'choices': [{'delta': {'content': piece}, 'finish_reason': None}]}) + '\n\n'
            await asyncio.sleep(0.3)
        yield 'data: {"choices":[{"delta":{},"finish_reason":"stop"}]}\n\ndata: [DONE]\n\n'
    return StreamingResponse(events(), media_type='text/event-stream')

if __name__ == '__main__':
    uvicorn.run(app, host='0.0.0.0', port=3000, access_log=False)
