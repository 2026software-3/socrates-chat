#!/usr/bin/env bash
# 顯示各服務的行程與健康檢查。全部健康回傳 0，否則回傳 1。
set -uo pipefail
# shellcheck source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

bad=0
for s in $SERVICES; do
  pid="$(service_pid "$s")"
  if ! is_alive "$pid"; then
    printf '%-8s 沒有在執行\n' "$s"
    bad=1
  elif service_healthy "$s"; then
    printf '%-8s 健康（pid %s）\n' "$s" "$pid"
  else
    printf '%-8s 行程在但健康檢查失敗（pid %s）；看 %s\n' "$s" "$pid" "$(log_file "$s")"
    bad=1
  fi
done
exit "$bad"
