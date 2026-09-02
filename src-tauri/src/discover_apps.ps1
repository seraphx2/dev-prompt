# Installed-application enumeration for dev-prompt's ">" app scope.
# Invoked by src/apps.rs via `powershell -Command -` (script on stdin). Emits a
# single compressed JSON array of
#   { name, exec, kind, args, icon, source, product, company, size }
# `__EXTRA_DIRS__` and `__ICON_CAP__` are substituted by the Rust caller.
# Rust does the noise filtering / dedupe; this script just gathers.

$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
Add-Type -AssemblyName System.Drawing | Out-Null

$iconDir = Join-Path $env:LOCALAPPDATA 'dev-prompt\cache\app-icons'
New-Item -ItemType Directory -Force -Path $iconDir | Out-Null
$script:iconCap = __ICON_CAP__
$script:iconCount = 0

function HashKey($s) {
  $sha = [System.Security.Cryptography.SHA1]::Create()
  -join ($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($s.ToLower())) | ForEach-Object { $_.ToString('x2') })
}

# base64 PNG for an executable's icon, disk-cached. "" when unavailable or capped.
function IconFor($path) {
  if (-not $path) { return '' }
  if (-not (Test-Path -LiteralPath $path)) { return '' }
  $ext = [IO.Path]::GetExtension($path).ToLower()
  if ($ext -ne '.exe' -and $ext -ne '.ico') { return '' }

  $cache = Join-Path $iconDir ((HashKey $path) + '.png')
  if (Test-Path -LiteralPath $cache) {
    try { return [Convert]::ToBase64String([IO.File]::ReadAllBytes($cache)) } catch { return '' }
  }
  if ($script:iconCount -ge $script:iconCap) { return '' }

  try {
    $ic = [System.Drawing.Icon]::ExtractAssociatedIcon($path)
    if (-not $ic) { return '' }
    $bmp = $ic.ToBitmap()
    $ms = New-Object IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bytes = $ms.ToArray()
    $ms.Dispose(); $bmp.Dispose(); $ic.Dispose()
    [IO.File]::WriteAllBytes($cache, $bytes)
    $script:iconCount++
    return [Convert]::ToBase64String($bytes)
  } catch { return '' }
}

$rows = New-Object System.Collections.ArrayList

# ProductName / CompanyName for an on-disk exe (used by Rust dedupe). Cheap —
# no icon work.
function MetaFor($path) {
  try {
    $vi = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($path)
    return @{ product = [string]$vi.ProductName; company = [string]$vi.CompanyName }
  } catch { return @{ product = ''; company = '' } }
}

# 1. Store apps (AppUserModelIDs) via Get-StartApps. Win32 .lnk apps also show
#    up here but are picked up with a real path in step 2.
foreach ($a in (Get-StartApps)) {
  if (-not $a.Name -or -not $a.AppID) { continue }
  if ($a.AppID -match '!') {
    [void]$rows.Add([pscustomobject]@{
      name = [string]$a.Name; exec = [string]$a.AppID; kind = 'aumid'
      args = ''; icon = ''; source = 'store'
    })
  }
}

# 2. Start Menu .lnk targets.
$sh = New-Object -ComObject WScript.Shell
$menus = @(
  (Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs'),
  (Join-Path $env:AppData 'Microsoft\Windows\Start Menu\Programs')
)
foreach ($m in $menus) {
  if (-not (Test-Path -LiteralPath $m)) { continue }
  foreach ($f in (Get-ChildItem -LiteralPath $m -Recurse -Filter *.lnk -File)) {
    try { $lnk = $sh.CreateShortcut($f.FullName) } catch { continue }
    $t = [string]$lnk.TargetPath
    if (-not $t) { continue }
    if ([IO.Path]::GetExtension($t).ToLower() -ne '.exe') { continue }
    $mi = MetaFor $t
    [void]$rows.Add([pscustomobject]@{
      name    = [IO.Path]::GetFileNameWithoutExtension($f.Name)
      exec    = $t
      kind    = 'exe'
      args    = [string]$lnk.Arguments
      icon    = (IconFor $t)
      source  = 'start-menu'
      product = $mi.product
      company = $mi.company
      size    = 0
    })
  }
}

# 3. Uninstall registry hives.
$unKeys = @(
  'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'
)
foreach ($kp in $unKeys) {
  foreach ($p in (Get-ItemProperty -Path $kp)) {
    if (-not $p.DisplayName) { continue }
    if ($p.SystemComponent -eq 1) { continue }
    if ($p.ParentKeyName) { continue }
    if ($p.ReleaseType -and $p.ReleaseType -match 'Update|Hotfix|Security') { continue }

    $exe = ''
    if ($p.DisplayIcon) {
      $di = ([string]$p.DisplayIcon).Trim('"')
      if ($di.Contains(',')) { $di = $di.Substring(0, $di.LastIndexOf(',')) }
      if ($di.ToLower().EndsWith('.exe') -and (Test-Path -LiteralPath $di)) { $exe = $di }
    }
    if (-not $exe -and $p.InstallLocation) {
      $loc = ([string]$p.InstallLocation).Trim('"')
      if ($loc -and (Test-Path -LiteralPath $loc)) {
        $cand = Get-ChildItem -LiteralPath $loc -Filter *.exe -File |
                Where-Object { $_.Name -notmatch '(?i)unins|setup|update|crash|helper' } |
                Sort-Object Length -Descending | Select-Object -First 1
        if ($cand) { $exe = $cand.FullName }
      }
    }
    if (-not $exe) { continue }

    $mi = MetaFor $exe
    [void]$rows.Add([pscustomobject]@{
      name = [string]$p.DisplayName; exec = $exe; kind = 'exe'
      args = ''; icon = (IconFor $exe); source = 'uninstall'
      product = $mi.product; company = $mi.company; size = 0
    })
  }
}

$reject = '(?i)unins|setup|update|crash|helper|redist|vcredist|elevate|squirrel|bootstrapper'

# 4a. Built-in per-user install root — requires a real FileDescription.
foreach ($d in @((Join-Path $env:LOCALAPPDATA 'Programs'))) {
  if (-not (Test-Path -LiteralPath $d)) { continue }
  foreach ($f in (Get-ChildItem -LiteralPath $d -Recurse -Depth 3 -Filter *.exe -File)) {
    if ($f.Name -match $reject) { continue }
    $vi = $f.VersionInfo
    $desc = [string]$vi.FileDescription
    if (-not $desc) { continue }
    [void]$rows.Add([pscustomobject]@{
      name = $desc; exec = $f.FullName; kind = 'exe'
      args = ''; icon = (IconFor $f.FullName); source = 'scan'
      product = [string]$vi.ProductName; company = [string]$vi.CompanyName
      size = [int64]$f.Length
    })
  }
}

# 4b. User-configured extra dirs — lenient: keep metadata-less binaries too
#     (Rust's per-folder "main binary" pick trims the rest).
foreach ($d in @(__EXTRA_DIRS__)) {
  if (-not $d -or -not (Test-Path -LiteralPath $d)) { continue }
  foreach ($f in (Get-ChildItem -LiteralPath $d -Recurse -Depth 3 -Filter *.exe -File)) {
    if ($f.Name -match $reject) { continue }
    $vi = $f.VersionInfo
    $desc = [string]$vi.FileDescription
    $nm = if ($desc) { $desc } else { [IO.Path]::GetFileNameWithoutExtension($f.Name) }
    [void]$rows.Add([pscustomobject]@{
      name = $nm; exec = $f.FullName; kind = 'exe'
      args = ''; icon = (IconFor $f.FullName); source = 'extra'
      product = [string]$vi.ProductName; company = [string]$vi.CompanyName
      size = [int64]$f.Length
    })
  }
}

# -InputObject (not the pipeline) so a single row still serialises as an array.
ConvertTo-Json -InputObject @($rows) -Compress -Depth 4
