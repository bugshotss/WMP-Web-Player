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

# The 32-bit helper that runs old WMP visualization plug-ins, in its own folder (its runtime is x86).
Write-Host "Publishing the 32-bit visualization helper"
dotnet publish (Join-Path $root "src\WebSkinPlayer.VisHost32\WebSkinPlayer.VisHost32.csproj") -c Release -r win-x86 --self-contained true `
    -o (Join-Path $publish "VisHost32") --nologo "-p:Version=$Version" -p:DebugType=none
if ($LASTEXITCODE) { throw "dotnet publish (VisHost32) failed" }

$zip = Join-Path $Out "WebSkinPlayer-$Version-win-x64-portable.zip"
Compress-Archive -Path (Join-Path $publish "*") -DestinationPath $zip

$iscc = (Get-Command iscc.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) {
    # Machine or per-user install, any version ("Inno Setup 6", "Inno Setup 7 (32-bit)", ...), newest first.
    $iscc = @("${env:ProgramFiles(x86)}", $env:ProgramFiles, "$env:LOCALAPPDATA\Programs") |
        ForEach-Object { Get-ChildItem $_ -Directory -Filter "Inno Setup *" -ErrorAction SilentlyContinue } |
        Sort-Object Name -Descending | ForEach-Object { Join-Path $_.FullName "ISCC.exe" } |
        Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $iscc) { throw "Inno Setup (ISCC.exe) not found. Install it with: winget install JRSoftware.InnoSetup" }
$numeric = ($Version -split '[-+]')[0]
& $iscc /Q "/DAppVersion=$Version" "/DAppVersionNumeric=$numeric" "/DSourceDir=$publish" "/DOutputDir=$Out" `
    "/DRepositoryUrl=$RepositoryUrl" (Join-Path $root "installer\WebSkinPlayer.iss")
if ($LASTEXITCODE) { throw "Inno Setup failed" }

Get-ChildItem $Out -File | ForEach-Object { "{0}  ({1:N1} MB)" -f $_.FullName, ($_.Length / 1MB) }
