#!/usr/bin/env bash
# 安裝 macOS（Apple Silicon）原生 cascade 需要的兩個 Python 虛擬環境與模型。不使用 Homebrew、不需要管理者權限、
# 不裝任何 CUDA 套件；所有東西都放在 CASCADE_MACOS_HOME（預設 ~/.cache/socrates-cascade-macos）。
#
#   venv-cascade  sglang-omni 0.2.0（與 NVIDIA 部署同版本）＋ sglang（上游的 Apple 安裝方式）＋ 轉接層
#   venv-voice    Whisper 與 Kokoro-82M：預設用 MLX（mlx-whisper、mlx-audio）；faster-whisper 與 PyTorch 版也裝，作為退路
#
# 用法：scripts/cascade-macos/install.sh [--skip-models]
# 需要：uv（pip install uv 或 brew install uv）、git、Python 3.12（uv 會自行取得）。
set -euo pipefail
# shellcheck source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

SGLANG_REPO="${SGLANG_REPO:-https://github.com/sgl-project/sglang.git}"
SGLANG_REF="${SGLANG_REF:-v0.5.23}"   # 與上游 install.sh 的預設相同；sglang-omni 0.2.0 在 macOS arm64 不會自己裝 sglang
SGLANG_DIR="$CASCADE_MACOS_HOME/sglang-$SGLANG_REF"
SKIP_MODELS=0
for arg in "$@"; do
  case "$arg" in
    --skip-models) SKIP_MODELS=1 ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) die "未知的選項：$arg" ;;
  esac
done

require_apple_silicon
command -v uv >/dev/null 2>&1 || die "找不到 uv；先安裝（pip install uv 或 brew install uv）"
command -v git >/dev/null 2>&1 || die "找不到 git"

# 網路不穩時的保護：下載逾時放寬並重試
export UV_HTTP_TIMEOUT="${UV_HTTP_TIMEOUT:-300}" UV_HTTP_RETRIES="${UV_HTTP_RETRIES:-10}"
unset DYLD_LIBRARY_PATH

mkdir -p "$CASCADE_MACOS_HOME"

make_venv() {
  if [ -x "$1/bin/python" ]; then
    log "沿用 $1"
  else
    uv venv --python 3.12 "$1"
  fi
}

fetch_sglang() {
  if [ -d "$SGLANG_DIR/.git" ]; then
    log "沿用 sglang 原始碼 $SGLANG_DIR"
    return
  fi
  log "下載 sglang $SGLANG_REF"
  local tmp="$SGLANG_DIR.tmp"
  rm -rf "$tmp" && mkdir -p "$tmp"
  git -C "$tmp" init -q
  git -C "$tmp" remote add origin "$SGLANG_REPO"
  git -C "$tmp" config http.lowSpeedLimit 0
  git -C "$tmp" config http.lowSpeedTime 999999
  local i=1
  until git -C "$tmp" fetch -q --depth 1 origin "refs/tags/$SGLANG_REF:refs/tags/$SGLANG_REF"; do
    [ "$i" -lt 6 ] || die "下載 sglang 失敗（網路？）"
    log "下載重試 $i"; sleep 10; i=$((i + 1))
  done
  git -C "$tmp" checkout -q --detach FETCH_HEAD
  mv "$tmp" "$SGLANG_DIR"
}

log "建立 venv-cascade"
make_venv "$CASCADE_VENV"
uv pip install --python "$CASCADE_VENV/bin/python" 'sglang-omni==0.2.0' opencc-python-reimplemented
fetch_sglang
# 上游把「非 CUDA」的專案設定放在 pyproject_other.toml；換掉暫存的 pyproject.toml 後以 all_mps 安裝。
[ -f "$SGLANG_DIR/python/pyproject_other.toml" ] || die "sglang 原始碼沒有 python/pyproject_other.toml（版本不符？）"
cp "$SGLANG_DIR/python/pyproject_other.toml" "$SGLANG_DIR/python/pyproject.toml"
log "安裝 sglang ${SGLANG_REF}（all_mps，編譯需要幾分鐘）"
SGLANG_BUILD_RUST_EXTS=none uv pip install --python "$CASCADE_VENV/bin/python" --prerelease=allow "$SGLANG_DIR/python[all_mps]"
# 補丁（使用者說話時 Dialog 不開口）；與 NVIDIA 映像用同一支，只認 sglang-omni 0.2.0 的原始碼
"$CASCADE_VENV/bin/python" "$REPO_ROOT/docker/cascade/patch_dialog.py"

log "建立 venv-voice"
make_venv "$VOICE_VENV"
uv pip install --python "$VOICE_VENV/bin/python" \
  faster-whisper fastapi uvicorn python-multipart numpy 'av>=14,<16' opencc-python-reimplemented \
  'kokoro==0.9.4' 'misaki[zh]==0.9.4' \
  'mlx-whisper==0.4.3' 'mlx-audio==0.6.5'   # 實測過的版本；見 README 的量測

log "驗證"
"$CASCADE_VENV/bin/sgl-omni" --help >/dev/null
"$CASCADE_VENV/bin/python" -c "import sglang, sglang_omni; print('sglang', sglang.__version__, '| sglang_omni', sglang_omni.__version__)"
"$VOICE_VENV/bin/python" -c "import torch; assert torch.backends.mps.is_available(), 'PyTorch MPS 不可用'; print('torch', torch.__version__, '| MPS 可用')"
"$VOICE_VENV/bin/python" -c "import mlx.core as mx, mlx_whisper, mlx_audio; assert mx.metal.is_available(), 'MLX Metal 不可用'; print('mlx', mx.__version__, '| Metal 可用')"

if [ "$SKIP_MODELS" -eq 0 ]; then
  log "預先下載模型（Whisper ${WHISPER_MODEL:-small}、Kokoro-82M；MLX 版與退路用的版本）到 Hugging Face 快取"
  "$VOICE_VENV/bin/python" - <<'PY'
import os
from faster_whisper import WhisperModel
from huggingface_hub import snapshot_download
from kokoro import KPipeline

name = os.environ.get("WHISPER_MODEL", "small")
snapshot_download(os.environ.get("WHISPER_MLX_MODEL") or f"mlx-community/whisper-{name}-mlx")
from mlx_audio.tts.utils import load_model
# 用實際載入的方式下載（只抓需要的檔案與聲線），並確認合成得出聲音
kokoro = load_model(os.environ.get("KOKORO_MLX_MODEL", "mlx-community/Kokoro-82M-bf16"))
assert list(kokoro.generate("你好", voice="zm_yunjian", lang_code="z", speed=1.0)), "MLX Kokoro produced no audio"
WhisperModel(name, device="cpu", compute_type="int8")
KPipeline(lang_code="z", device="cpu").load_voice("zm_yunjian")
print("models ready")
PY
fi

log "完成。接著：export CASCADE_LLM_TOKEN=...；scripts/cascade-macos/start.sh"
