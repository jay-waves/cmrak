$ErrorActionPreference = 'Stop'
if (-not [Environment]::Is64BitOperatingSystem -or $env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { throw 'Only Windows x64 is supported.' }
$command = Get-Command cmrak.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
if ($command) {
    $target = $command.Path
    $installDir = Split-Path -Parent $target
} else {
    $installDir = if ($env:INSTALL_DIR) { $env:INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\cmrak' }
    $installDir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($installDir)
    if ($installDir.Contains(';')) { throw 'The install directory cannot contain a semicolon.' }
    $target = Join-Path $installDir 'cmrak.exe'
}
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
    if (Test-Path -LiteralPath $target -PathType Leaf) {
        Get-Process -Name cmrak -ErrorAction SilentlyContinue |
            Where-Object { $_.Path -eq $target } |
            ForEach-Object {
                if (-not $_.HasExited) {
                    Stop-Process -InputObject $_ -Force
                    if (-not $_.WaitForExit(5000)) { throw "Process $($_.Id) did not stop." }
                }
            }
        [IO.File]::Replace($temp, $target, [NullString]::Value)
    } else {
        Move-Item -LiteralPath $temp -Destination $target
    }
} finally {
    if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp -Force }
}

if (-not $command) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $normalizedDir = $installDir.TrimEnd('\', '/')
    if (-not ($userPath -split ';' | Where-Object { $_.TrimEnd('\', '/') -eq $normalizedDir })) {
        [Environment]::SetEnvironmentVariable('Path', ((@($installDir, $userPath) | Where-Object { $_ }) -join ';'), 'User')
    }
    if (-not ($env:Path -split ';' | Where-Object { $_.TrimEnd('\', '/') -eq $normalizedDir })) {
        $env:Path = "$installDir;$env:Path"
    }
}

Write-Host "Installed cmrak to $target"
& $target --version
