<#
.SYNOPSIS
  Builds a release into artifacts\: a self-contained publish (no .NET install needed on the
  target PC), a portable zip, and the per-user Setup.exe. Used locally and by the GitHub workflow.
.EXAMPLE
  .\tools\Build-Release.ps1
  .\tools\Build-Release.ps1 -Version 0.2.0 -RepositoryUrl https://github.com/you/webskin-player
#>
param(
    [string]$Version,
    [string]$RepositoryUrl = "",
    [string]$Out = (Join-Path $PSScriptRoot "..\artifacts")
)
$ErrorActionPreference = "Stop"
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$project = Join-Path $root "src\WebSkinPlayer\WebSkinPlayer.csproj"
if (-not $Version) { $Version = (([xml](Get-Content $project -Raw)).Project.PropertyGroup | ForEach-Object { $_.Version } | Where-Object { $_ } | Select-Object -First 1) }
$Out = [IO.Path]::GetFullPath($Out)
$publish = Join-Path $Out "publish"
if (Test-Path $Out) { Remove-Item $Out -Recurse -Force }
New-Item -ItemType Directory -Force $publish | Out-Null

Write-Host "Publishing WebSkin Player $Version (self-contained, win-x64)"
dotnet publish $project -c Release -r win-x64 --self-contained true -o $publish --nologo `
    "-p:Version=$Version" "-p:RepositoryUrl=$RepositoryUrl" `
    -p:SatelliteResourceLanguages=en -p:DebugType=embedded
if ($LASTEXITCODE) { throw "dotnet publish failed" }

$zip = Join-Path $Out "WebSkinPlayer-$Version-win-x64-portable.zip"
Compress-Archive -Path (Join-Path $publish "*") -DestinationPath $zip

$iscc = (Get-Command iscc.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) {
    $iscc = @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
              "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $iscc) { throw "Inno Setup 6 (ISCC.exe) not found. Install it with: winget install JRSoftware.InnoSetup" }
$numeric = ($Version -split '[-+]')[0]
& $iscc /Q "/DAppVersion=$Version" "/DAppVersionNumeric=$numeric" "/DSourceDir=$publish" "/DOutputDir=$Out" `
    "/DRepositoryUrl=$RepositoryUrl" (Join-Path $root "installer\WebSkinPlayer.iss")
if ($LASTEXITCODE) { throw "Inno Setup failed" }

Get-ChildItem $Out -File | ForEach-Object { "{0}  ({1:N1} MB)" -f $_.FullName, ($_.Length / 1MB) }
