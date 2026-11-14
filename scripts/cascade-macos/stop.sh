#!/usr/bin/env bash
# 停止 macOS 原生 cascade 的所有服務（反向順序），只動這個腳本啟動的行程。
set -euo pipefail
# shellcheck source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

order=""
for s in $SERVICES; do order="$s $order"; done
for s in $order; do
  if is_alive "$(service_pid "$s")"; then
    log "停止 $s"
  fi
  stop_service "$s"
done
log "已停止"
