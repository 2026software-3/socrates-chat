# Kokoro 與 MeloTTS CPU 初步測試（2026-11-12）

機器：Intel Core i7-13620H、15 GiB RAM；Python 3.11.15、PyTorch 2.15.0+cpu；`torch.set_num_threads(4)`。Kokoro 0.9.4、MeloTTS commit `209145371cff8fc3bd60d7be902ea69cbdb7965a`。各語言單獨啟動程序，模型已快取，依序執行，其他服務仍在運作。每項僅一次，結果不代表分布或容量。時間從呼叫推論到第一個完整音訊區塊，不含模型初始化；MeloTTS 此測試回傳完整音訊，並非串流 TTFA。RSS 是從程序啟動至輸出後的程序峰值，含模型與程式庫，不是全機 RAM；耗時比音訊秒數為 RTF。

| 模型 | 語言 | 峰值 RSS MiB | 首段／完成秒數 | 音訊秒數 | RTF |
| --- | --- | ---: | ---: | ---: | ---: |
| Kokoro | 中文 zf_xiaobei | 1314 | 1.52 | 4.88 | 0.31 |
| Kokoro | 英文 af_heart | 1277 | 0.87 | 3.68 | 0.24 |
| Kokoro | 西文 ef_dora | 1277 | 0.91 | 4.00 | 0.23 |
| MeloTTS | 中文 ZH | 2956 | 2.99 | 3.15 | 0.95 |
| MeloTTS | 英文 EN | 2264 | 2.17 | 4.19 | 0.52 |
| MeloTTS | 西文 ES | 1520 | 1.60 | 4.65 | 0.34 |

文字：`什麼理由會讓你改變對這個問題的看法？`、`What reasons might lead you to change your mind about this question?`、`¿Qué razones podrían hacerte cambiar de opinión sobre esta pregunta?`。語音檔在 Windows `D:\socrates-voice-tts-test-20261112`。聲線、音訊秒數不同；未評分自然度與正確性，中文使用繁體原文。

GPU 未測：RTX 4050 6141 MiB 當時已有運行中的 cascade TTS 佔約 4837 MiB。未停止現有服務，也未測併發或長時間運作。既有 Qwen 5512 MiB 紀錄未標明是裝置使用量或程序峰值，不能直接與 CPU RSS 比較。

`kokoro_cpu.py` 和 `melo_cpu.py` 為量測用腳本，依序接受語言代碼、（Kokoro 的聲線）、文字；`VOICE_OUTPUT` 可指定 WAV 檔案，`HF_HOME` 指定權重快取。MeloTTS 原始碼需在 `PYTHONPATH`；須先另行安裝各自依賴及下載模型。峰值與延遲受快取、競爭負載、版本與語音長度影響，部署決策前應重新測試。

## GPU 單請求測試（2026-11-12）

暫停既有 cascade TTS 後，使用 RTX 4050 Laptop GPU 6141 MiB，裝置測試前為 0 MiB。隔離容器重用 `socrates-cascade:latest` 的 PyTorch 2.14.1+cu130，Python 3.12.3；各語言在獨立容器、單一程序、FP32、4 個 CPU 執行緒執行。MeloTTS 的 transformers 為 4.44.2；CPU 測試用 4.27.4、PyTorch 2.15.0+cpu，因此 CPU 與 GPU 效能不完全同設定。每項一次，使用已快取權重；首輪未暖機，延遲包含部分文字前處理及延遲載入，不含模型初始化；MeloTTS 回傳整段音訊，不是串流 TTFA。

| 模型 | 語言 | allocated 峰值 MiB | reserved 峰值 MiB | nvidia-smi 裝置採樣峰值 MiB | CPU 峰值 RSS MiB | 首段／完成秒數 | RTF |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Kokoro | 中 | 644 | 684 | 799 | 2035 | 1.92 | 0.39 |
| Kokoro | 英 | 632 | 664 | 779 | 2028 | 1.22 | 0.33 |
| Kokoro | 西 | 641 | 740 | 855 | 1967 | 0.96 | 0.24 |
| MeloTTS | 中 | 966 | 1056 | 1161 | 2551 | 5.10 | 1.67 |
| MeloTTS | 英 | 774 | 882 | 987 | 2522 | 2.24 | 0.54 |
| MeloTTS | 西 | 402 | 482 | 511 | 2351 | 1.22 | 0.26 |

PyTorch 峰值涵蓋模型載入與推論；reserved 含 allocated，不應相加。裝置記憶體由 `nvidia-smi` 週期採樣（約 100 ms 加查詢耗時），可能漏掉短暫尖峰，也包含裝置上所有程序。Qwen 的 5512 MiB 原紀錄未註明測量方法，不能假設與此數字相同。未做服務鏈路、預熱後多次推論、2／4 並發或長時間測試。GPU 音檔以 `*-gpu-*.wav` 放在同一 D 槽目錄。
