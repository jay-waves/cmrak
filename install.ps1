$ErrorActionPreference = 'Stop'
if (-not [Environment]::Is64BitOperatingSystem -or $env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { throw 'Only Windows x64 is supported.' }
$installDir = if ($env:INSTALL_DIR) { $env:INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\cmrak' }
New-Item -ItemType Directory -Force -Path $installDir | Out-Null
$target = Join-Path $installDir 'cmrak.exe'
$temp = Join-Path $installDir ([IO.Path]::GetRandomFileName())
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -UseBasicParsing -Headers @{ 'Cache-Control' = 'no-cache' } -Uri 'https://github.com/jay-waves/cmrak/releases/download/0.5.0/cmrak-windows-x64.exe' -OutFile $temp
    Move-Item -Force -LiteralPath $temp -Destination $target
} finally {
    if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp -Force }
}

Write-Host "Installed cmrak to $target"
Write-Host "PATH was not modified. Add $installDir to PATH to run cmrak from any directory."
