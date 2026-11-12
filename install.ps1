$ErrorActionPreference = 'Stop'
if (-not [Environment]::Is64BitOperatingSystem -or $env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { throw 'Only Windows x64 is supported.' }
$command = Get-Command cmrak.exe -ErrorAction SilentlyContinue
if ($command) { $target = $command.Source; $installDir = Split-Path -Parent $target } else { $installDir = if ($env:INSTALL_DIR) { $env:INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\cmrak' }; $target = Join-Path $installDir 'cmrak.exe' }
New-Item -ItemType Directory -Force -Path $installDir | Out-Null
$temp = Join-Path $installDir ([IO.Path]::GetRandomFileName())
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $cacheBust = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $headers = @{ 'Accept' = 'application/vnd.github+json'; 'User-Agent' = 'cmrak-installer'; 'Cache-Control' = 'no-cache' }
    $release = Invoke-RestMethod -Headers $headers -Uri "https://api.github.com/repos/jay-waves/cmrak/releases/latest?cachebust=$cacheBust"
    $asset = $release.assets | Where-Object { $_.name -eq 'cmrak-windows-x64.exe' } | Select-Object -First 1
    if (-not $asset) { throw "Latest release $($release.tag_name) has no cmrak-windows-x64.exe asset." }
    Invoke-WebRequest -UseBasicParsing -Headers $headers -Uri "$($asset.browser_download_url)?cachebust=$cacheBust" -OutFile $temp
    Move-Item -Force -LiteralPath $temp -Destination $target
} finally {
    if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp -Force }
}

Write-Host "Installed cmrak to $target"
& $target --version
Write-Host "Add $installDir to PATH: $env:PATH = '$installDir;' + $env:PATH"
