$ErrorActionPreference = "Stop"

function Assert-True([bool] $Condition, [string] $Message) {
  if (-not $Condition) {
    throw $Message
  }
}

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$temporary = Join-Path ([System.IO.Path]::GetTempPath()) "appd-installer-test-$([guid]::NewGuid())"
$fixtures = Join-Path $temporary "fixtures"
$global:AppdInstallerFixtures = $fixtures
$global:AppdInstallerDownloads = @()

try {
  $cli = Join-Path $temporary "cli"
  New-Item -ItemType Directory -Force -Path $cli, $fixtures | Out-Null
  Set-Content -LiteralPath (Join-Path $cli "appd.exe") -Value "appd"
  Compress-Archive -Path (Join-Path $cli "appd.exe") -DestinationPath (Join-Path $fixtures "appd-cli-windows-x64.zip")

  $targets = @(
    "android-arm64",
    "ios-arm64",
    "ios-simulator-arm64",
    "ios-simulator-x64",
    "macos-arm64",
    "macos-x64",
    "windows-x64"
  )
  foreach ($target in $targets) {
    $pack = Join-Path $temporary $target
    New-Item -ItemType Directory -Force -Path $pack | Out-Null
    Set-Content -LiteralPath (Join-Path $pack "target-pack.json") -Value "{`"target`":`"$target`"}"
    & tar -czf (Join-Path $fixtures "appd-target-pack-$target.tar.gz") -C $pack .
    if ($LASTEXITCODE -ne 0) {
      throw "failed to create $target fixture"
    }
  }

  function Invoke-RestMethod {
    param([string] $Uri, [hashtable] $Headers)
    $global:AppdInstallerDownloads += $Uri
    @([pscustomobject]@{ tag_name = "pre.2" })
  }

  function Invoke-WebRequest {
    param(
      [string] $Uri,
      [hashtable] $Headers,
      [string] $OutFile,
      [switch] $UseBasicParsing
    )
    $global:AppdInstallerDownloads += $Uri
    Copy-Item -LiteralPath (Join-Path $global:AppdInstallerFixtures ([System.IO.Path]::GetFileName($Uri))) -Destination $OutFile
  }

  $installRoot = Join-Path $temporary "home/.local"
  $binDirectory = Join-Path $installRoot "bin"
  $obsolete = Join-Path $installRoot "share/appd/target-packs/obsolete"
  New-Item -ItemType Directory -Force -Path $binDirectory, $obsolete | Out-Null
  Set-Content -LiteralPath (Join-Path $binDirectory "appd.exe") -Value "old"
  Set-Content -LiteralPath (Join-Path $obsolete "target-pack.json") -Value "old"

  $output = & (Join-Path $repositoryRoot "scripts/install.ps1") -InstallRoot $installRoot | Out-String

  $installedCli = Join-Path $binDirectory "appd.exe"
  Assert-True (Test-Path -LiteralPath $installedCli -PathType Leaf) "CLI was not installed"
  Assert-True ((Get-Content -LiteralPath $installedCli -Raw).Contains("appd")) "CLI was not replaced"
  foreach ($target in $targets) {
    Assert-True (Test-Path -LiteralPath (Join-Path $installRoot "share/appd/target-packs/$target/target-pack.json") -PathType Leaf) "$target was not installed"
    Assert-True ((@($global:AppdInstallerDownloads -match "/appd-target-pack-$target.tar.gz$")).Count -gt 0) "$target was not downloaded"
  }
  Assert-True (-not (Test-Path -LiteralPath $obsolete)) "obsolete target pack was retained"
  Assert-True ((@($global:AppdInstallerDownloads -match "/appd-cli-windows-x64.zip$")).Count -gt 0) "Windows CLI was not downloaded"
  Assert-True ($output.Contains("Installed appd pre.2")) "release tag was not reported"
  Assert-True ($output.Contains("Add $(Join-Path $installRoot 'bin') to PATH")) "PATH instruction was not reported"
} finally {
  Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
  Remove-Item Function:\Invoke-RestMethod -ErrorAction SilentlyContinue
  Remove-Item Function:\Invoke-WebRequest -ErrorAction SilentlyContinue
  Remove-Variable AppdInstallerFixtures -Scope Global -ErrorAction SilentlyContinue
  Remove-Variable AppdInstallerDownloads -Scope Global -ErrorAction SilentlyContinue
}
