param(
  [string] $InstallRoot = (Join-Path $HOME ".local")
)

$ErrorActionPreference = "Stop"
$repository = "mantty/appd"
$api = "https://api.github.com/repos/$repository"
$targets = @(
  "android-arm64",
  "ios-arm64",
  "ios-simulator-arm64",
  "ios-simulator-x64",
  "macos-arm64",
  "macos-x64",
  "windows-x64"
)

if ($env:OS -ne "Windows_NT" -or -not [Environment]::Is64BitOperatingSystem) {
  throw "appd requires 64-bit Windows"
}
if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
  throw "InstallRoot must not be empty"
}
if (-not (Get-Command tar -ErrorAction SilentlyContinue)) {
  throw "tar is required"
}

$apiHeaders = @{
  Accept = "application/vnd.github+json"
  "X-GitHub-Api-Version" = "2022-11-28"
}

$releases = @(Invoke-RestMethod -Uri "$api/releases?per_page=1" -Headers $apiHeaders)
if ($releases.Count -eq 0) {
  throw "no appd release was found"
}
$tag = $releases[0].tag_name
if ($tag -notmatch "^[A-Za-z0-9._-]+$") {
  throw "unsupported release tag: $tag"
}

$releaseUrl = "https://github.com/$repository/releases/download/$tag"
$temporary = Join-Path ([System.IO.Path]::GetTempPath()) "appd-install-$([guid]::NewGuid())"
$stagedCli = $null
$stagedTargetPacks = $null

try {
  $cliDirectory = Join-Path $temporary "cli"
  $targetPacks = Join-Path $temporary "target-packs"
  New-Item -ItemType Directory -Force -Path $cliDirectory, $targetPacks | Out-Null

  Write-Output "Downloading appd $tag for windows-x64..."
  $cliArchive = Join-Path $temporary "appd-cli-windows-x64.zip"
  Invoke-WebRequest -UseBasicParsing -Uri "$releaseUrl/appd-cli-windows-x64.zip" -OutFile $cliArchive
  Expand-Archive -LiteralPath $cliArchive -DestinationPath $cliDirectory
  $cli = Join-Path $cliDirectory "appd.exe"
  if (-not (Test-Path -LiteralPath $cli -PathType Leaf)) {
    throw "CLI archive does not contain appd.exe"
  }

  foreach ($target in $targets) {
    Write-Output "Downloading target pack $target..."
    $archive = Join-Path $temporary "appd-target-pack-$target.tar.gz"
    $destination = Join-Path $targetPacks $target
    New-Item -ItemType Directory -Force -Path $destination | Out-Null
    Invoke-WebRequest -UseBasicParsing -Uri "$releaseUrl/appd-target-pack-$target.tar.gz" -OutFile $archive
    & tar -xzf $archive -C $destination
    if ($LASTEXITCODE -ne 0) {
      throw "failed to extract $target"
    }
    if (-not (Test-Path -LiteralPath (Join-Path $destination "target-pack.json") -PathType Leaf)) {
      throw "$target archive does not contain target-pack.json"
    }
  }

  $InstallRoot = [System.IO.Path]::GetFullPath($InstallRoot)
  $binDirectory = Join-Path $InstallRoot "bin"
  $shareDirectory = Join-Path $InstallRoot "share/appd"
  $targetPackDirectory = Join-Path $shareDirectory "target-packs"
  New-Item -ItemType Directory -Force -Path $binDirectory, $shareDirectory | Out-Null

  $stagedCli = Join-Path $binDirectory ".appd-install-$PID.exe"
  $stagedTargetPacks = Join-Path $shareDirectory ".target-packs-install-$PID"
  Copy-Item -LiteralPath $cli -Destination $stagedCli
  Copy-Item -LiteralPath $targetPacks -Destination $stagedTargetPacks -Recurse
  Remove-Item -LiteralPath $targetPackDirectory -Recurse -Force -ErrorAction SilentlyContinue
  Move-Item -LiteralPath $stagedTargetPacks -Destination $targetPackDirectory
  $stagedTargetPacks = $null
  $installedCli = Join-Path $binDirectory "appd.exe"
  Remove-Item -LiteralPath $installedCli -Force -ErrorAction SilentlyContinue
  Move-Item -LiteralPath $stagedCli -Destination $installedCli
  $stagedCli = $null

  Write-Output ""
  Write-Output "Installed appd $tag in $binDirectory"
  Write-Output "Installed target packs in $targetPackDirectory"
  if (@($env:Path -split [System.IO.Path]::PathSeparator) -notcontains $binDirectory) {
    Write-Output "Add $binDirectory to PATH, then open a new terminal."
  }
  Write-Output "Run appd targets to verify the installation."
} finally {
  Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
  if ($stagedCli) {
    Remove-Item -LiteralPath $stagedCli -Force -ErrorAction SilentlyContinue
  }
  if ($stagedTargetPacks) {
    Remove-Item -LiteralPath $stagedTargetPacks -Recurse -Force -ErrorAction SilentlyContinue
  }
}
