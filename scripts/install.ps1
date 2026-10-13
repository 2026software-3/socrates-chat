# 一鍵安裝（Windows PowerShell 5.1 以上）：與 install.sh 相同流程。
# 檢查 Docker、產生 .env、建置並啟動整個系統（資料庫＋後端＋前端）。
#
# 用法：
#   powershell -ExecutionPolicy RemoteSigned -File scripts\install.ps1
# 非互動：先以環境變數提供下列必填值即可略過提問。
#   GOOGLE_CLIENT_ID  GOOGLE_CLIENT_SECRET  ADMIN_EMAILS  OPENAI_API_KEY  OPENAI_MODEL
# 可選：APP_BASE_URL（預設 http://localhost:3000）、ADMIN_INITIAL_PASSWORD（預設隨機產生）
#
# 已有 .env 時不會覆寫，直接使用它。
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

function Fail($msg) {
    Write-Error "錯誤：$msg"
    exit 1
}

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) { Fail '找不到 docker，請先安裝 Docker Desktop' }
& docker compose version *> $null
if ($LASTEXITCODE -ne 0) { Fail '需要 Docker Compose v2（docker compose）' }
& docker info *> $null
if ($LASTEXITCODE -ne 0) { Fail '無法連線到 Docker daemon，請確認 Docker 已啟動' }

function Read-Value([string]$Name, [string]$Prompt, [bool]$Secret = $false) {
    $value = [Environment]::GetEnvironmentVariable($Name)
    if ([string]::IsNullOrEmpty($value)) {
        if (-not [Environment]::UserInteractive -or [Console]::IsInputRedirected) {
            Fail "非互動環境請先設定環境變數 $Name"
        }
        while ([string]::IsNullOrEmpty($value)) {
            if ($Secret) {
                $secure = Read-Host -Prompt $Prompt -AsSecureString
                $value = [Runtime.InteropServices.Marshal]::PtrToStringBSTR(
                    [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure))
            } else {
                $value = Read-Host -Prompt $Prompt
            }
        }
    }
    return $value
}

function New-RandomPassword {
    $chars = [char[]]'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789'
    $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
    $bytes = New-Object byte[] 20
    $rng.GetBytes($bytes)
    -join ($bytes | ForEach-Object { $chars[$_ % $chars.Length] })
}

$generatedPassword = $null
$baseUrl = if ($env:APP_BASE_URL) { $env:APP_BASE_URL } else { 'http://localhost:3000' }

if (Test-Path .env) {
    Write-Host '使用既有的 .env（不覆寫）'
    $line = Select-String -Path .env -Pattern '^APP_BASE_URL=(.+)$' | Select-Object -First 1
    if ($line) { $baseUrl = $line.Matches[0].Groups[1].Value }
    $adminLine = Select-String -Path .env -Pattern '^ADMIN_EMAILS=(.+)$' | Select-Object -First 1
    $adminEmails = if ($adminLine) { $adminLine.Matches[0].Groups[1].Value } else { '' }
} else {
    $values = [ordered]@{
        APP_BASE_URL           = $baseUrl
        GOOGLE_CLIENT_ID       = Read-Value 'GOOGLE_CLIENT_ID' 'Google OAuth client ID'
        GOOGLE_CLIENT_SECRET   = Read-Value 'GOOGLE_CLIENT_SECRET' 'Google OAuth client secret' $true
        ADMIN_EMAILS           = Read-Value 'ADMIN_EMAILS' '管理者信箱（多個以逗號分隔）'
        ADMIN_INITIAL_PASSWORD = if ($env:ADMIN_INITIAL_PASSWORD) { $env:ADMIN_INITIAL_PASSWORD } else { New-RandomPassword }
        OPENAI_API_KEY         = Read-Value 'OPENAI_API_KEY' 'OpenAI API key' $true
        OPENAI_MODEL           = Read-Value 'OPENAI_MODEL' 'OpenAI 模型名稱'
    }
    $generatedPassword = $values.ADMIN_INITIAL_PASSWORD
    $adminEmails = $values.ADMIN_EMAILS

    $lines = Get-Content .env.example -Encoding UTF8 | ForEach-Object {
        $out = $_
        foreach ($key in $values.Keys) {
            if ($_ -match "^$key=") { $out = "$key=$($values[$key])"; break }
        }
        $out
    }
    # 不帶 BOM 的 UTF-8，換行用 LF，避免 .env 值尾端多出 CR
    [IO.File]::WriteAllText((Join-Path (Get-Location) '.env'), (($lines -join "`n") + "`n"),
        (New-Object Text.UTF8Encoding($false)))
    Write-Host '已建立 .env'
}

Write-Host '建置並啟動服務（首次建置需要數分鐘）...'
& docker compose --profile app up --build -d
if ($LASTEXITCODE -ne 0) { Fail 'docker compose 啟動失敗' }

Write-Host '等待服務就緒...'
$ready = $false
for ($i = 0; $i -lt 60; $i++) {
    try {
        Invoke-WebRequest -Uri 'http://127.0.0.1:3000/health' -UseBasicParsing -TimeoutSec 3 | Out-Null
        $ready = $true
        break
    } catch {
        Start-Sleep -Seconds 2
    }
}
if (-not $ready) { Fail '服務未在時限內就緒，請用 docker compose logs app 查看原因' }

Write-Host ''
Write-Host "安裝完成：$baseUrl"
if ($generatedPassword) {
    Write-Host "管理者帳號：$(($adminEmails -split ',')[0])"
    Write-Host "初始密碼：$generatedPassword（首次登入須更改；亦寫在 .env）"
}
Write-Host '停止：docker compose --profile app down'
