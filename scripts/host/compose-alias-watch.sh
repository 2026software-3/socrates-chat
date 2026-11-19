#!/usr/bin/env bash
# compose 重建網路後，沒被重建的容器（db、cascade、cloudflared 等）只是停了再啟，重新接上新網路時會：
#   1. 失去服務名稱別名（例如 db、cascade、proxy），其他容器就解析不到它們；
#   2. 失去固定位址（例如 app 的 ipv4_address），改拿動態位址。
# 這支腳本在背景盯著，發現就補回，不重啟容器。
#
# 用法：compose-alias-watch.sh <compose 專案名> <持續秒數> [<服務>=<固定位址> ...]
#   例：scripts/host/compose-alias-watch.sh socrates-ntnu 1800 app=172.29.250.10
# 只看帶有 com.docker.compose.project=<專案名> 標籤的容器與以 <專案名>_ 開頭的網路；其他專案（例如接進來的
# staging-proxy）不碰。需要 docker 權限（msi_cubi 上的 ray520 在 docker 群組）。同一個專案同時只會有一個在跑。
# 每 0.3 秒檢查一次；補的時候 disconnect／connect 約 0.1 秒。
set -u

if [ "$#" -lt 2 ]; then
  echo "用法：compose-alias-watch.sh <compose 專案名> <持續秒數> [<服務>=<固定位址> ...]" >&2
  exit 2
fi
project="$1"
secs="$2"
shift 2
case "$secs" in '' | *[!0-9]*) echo "持續秒數要是整數" >&2; exit 2 ;; esac

declare -A want_ip=()
for pair in "$@"; do
  case "$pair" in
    *=*.*.*.*) want_ip["${pair%%=*}"]="${pair#*=}" ;;
    *) echo "固定位址要寫成 <服務>=<IPv4>：$pair" >&2; exit 2 ;;
  esac
done

# 同一個專案只跑一個，避免兩個腳本互相搶著重接。
exec 8>"/tmp/compose-alias-watch.$project.lock"
if ! flock -n 8; then
  echo "已經有一個 $project 的 compose-alias-watch 在跑" >&2
  exit 1
fi

ts() { date +%H:%M:%S.%N | cut -c1-12; }

end=$((SECONDS + secs))
echo "$(ts) 開始盯 $project（$secs 秒）"
while [ "$SECONDS" -lt "$end" ]; do
  ids=$(docker ps -q --filter "label=com.docker.compose.project=$project" 2>/dev/null)
  if [ -n "$ids" ]; then
    # 每個容器一行：容器名|服務名|網路=別名,別名,=設定的固定位址=實際位址;網路=...
    # shellcheck disable=SC2086
    docker inspect $ids --format '{{.Name}}|{{index .Config.Labels "com.docker.compose.service"}}|{{range $n, $v := .NetworkSettings.Networks}}{{$n}}={{range $v.Aliases}}{{.}},{{end}}={{with $v.IPAMConfig}}{{.IPv4Address}}{{end}}={{$v.IPAddress}};{{end}}' 2>/dev/null |
      while IFS='|' read -r name svc nets; do
        name="${name#/}"
        [ -n "$svc" ] || continue
        IFS=';' read -r -a entries <<<"$nets"
        for e in "${entries[@]}"; do
          [ -n "$e" ] || continue
          IFS='=' read -r net aliases cfg_ip cur_ip <<<"$e"
          case "$net" in "${project}_"*) ;; *) continue ;; esac
          target_ip="${want_ip[$svc]:-$cfg_ip}"
          need_alias=0
          need_ip=0
          case ",$aliases" in *",$svc,"*) ;; *) need_alias=1 ;; esac
          if [ -n "${want_ip[$svc]:-}" ] && [ "$cur_ip" != "${want_ip[$svc]}" ]; then need_ip=1; fi
          [ "$need_alias" -eq 1 ] || [ "$need_ip" -eq 1 ] || continue
          echo "$(ts) $name 在 $net：別名缺少=$need_alias 位址不符=$need_ip（現在 ${cur_ip:--}），補上"
          ipopt=()
          [ -n "$target_ip" ] && ipopt=(--ip "$target_ip")
          if docker network disconnect "$net" "$name" && docker network connect --alias "$svc" "${ipopt[@]}" "$net" "$name"; then
            echo "$(ts) 已補上 $name → 別名 $svc${target_ip:+、位址 $target_ip}"
          else
            echo "$(ts) 補失敗：$name（$net）"
          fi
        done
      done
  fi
  sleep 0.3
done
echo "$(ts) 結束"
