# Installed-application enumeration for dev-prompt's ">" app scope.
# Invoked by src/apps.rs via `powershell -Command -` (script on stdin). Emits a
# single compressed JSON array of
#   { origin, name, exec, kind, args, icon, source, product, company, size }
# `__PARAMS__` (JSON, see `GatherParams` in apps.rs) and `__ICON_CAP__` /
# `__ICON_DIR__` are substituted by the Rust caller.
#
# The same script does a full scan and a partial one: each source in the params
# is either `null` (gather all of it) or a list of the only items to gather —
# so an incremental update after one install goes through exactly the code a
# full rescan does. `origin` says which item each row came from, so the caller
# can replace just that item's rows later.
# Rust does the noise filtering / dedupe; this script just gathers.
# (`$Params`, not `$P`: PowerShell variable names ignore case, and `$p` is
# the loop variable for registry entries below.)

$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
Add-Type -AssemblyName System.Drawing | Out-Null

$Params = @'
__PARAMS__
'@ | ConvertFrom-Json

$iconDir = '__ICON_DIR__'
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
#    up here but are picked up with a real path in step 2. All or nothing:
#    there's no asking for one package.
if ($Params.store) {
  foreach ($a in (Get-StartApps)) {
    if (-not $a.Name -or -not $a.AppID) { continue }
    if ($a.AppID -match '!') {
      [void]$rows.Add([pscustomobject]@{
        origin = 'store'
        name = [string]$a.Name; exec = [string]$a.AppID; kind = 'aumid'
        args = ''; icon = ''; source = 'store'
      })
    }
  }
}

# 2. Start Menu .lnk targets. `menus` = null: every shortcut under every root;
#    otherwise just those shortcuts / folders (a missing one yields nothing).
$sh = New-Object -ComObject WScript.Shell
function AddShortcut($file) {
  try { $lnk = $sh.CreateShortcut($file) } catch { return }
  $t = [string]$lnk.TargetPath
  if (-not $t) { return }
  if ([IO.Path]::GetExtension($t).ToLower() -ne '.exe') { return }
  $mi = MetaFor $t
  [void]$rows.Add([pscustomobject]@{
    origin  = 'lnk:' + $file
    name    = [IO.Path]::GetFileNameWithoutExtension($file)
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
$menuItems = if ($null -eq $Params.menus) { $Params.menuRoots } else { $Params.menus }
foreach ($m in $menuItems) {
  if (-not $m -or -not (Test-Path -LiteralPath $m)) { continue }
  if (Test-Path -LiteralPath $m -PathType Leaf) {
    if ($m.ToLower().EndsWith('.lnk')) { AddShortcut $m }
    continue
  }
  foreach ($f in (Get-ChildItem -LiteralPath $m -Recurse -Filter *.lnk -File)) {
    AddShortcut $f.FullName
  }
}

# 3. Uninstall registry entries. `uninstallRoots` is [{ tag, path }]; an entry's
#    origin is `uninstall:<tag>\<subkey>`. `uninstall` = null: every subkey;
#    otherwise just those `<tag>\<subkey>` ids.
function AddUninstall($p, $origin) {
  if (-not $p.DisplayName) { return }
  if ($p.SystemComponent -eq 1) { return }
  if ($p.ParentKeyName) { return }
  if ($p.ReleaseType -and $p.ReleaseType -match 'Update|Hotfix|Security') { return }

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
  if (-not $exe) { return }

  $mi = MetaFor $exe
  [void]$rows.Add([pscustomobject]@{
    origin = $origin
    name = [string]$p.DisplayName; exec = $exe; kind = 'exe'
    args = ''; icon = (IconFor $exe); source = 'uninstall'
    product = $mi.product; company = $mi.company; size = 0
  })
}
foreach ($r in $Params.uninstallRoots) {
  if ($null -eq $Params.uninstall) {
    foreach ($p in (Get-ItemProperty -Path ('Registry::' + $r.path + '\*'))) {
      AddUninstall $p ('uninstall:' + $r.tag + '\' + $p.PSChildName)
    }
  } else {
    foreach ($id in $Params.uninstall) {
      $prefix = $r.tag + '\'
      if (-not $id.StartsWith($prefix)) { continue }
      $sub = $id.Substring($prefix.Length)
      $p = Get-ItemProperty -LiteralPath ('Registry::' + $r.path + '\' + $sub)
      if ($p) { AddUninstall $p ('uninstall:' + $id) }
    }
  }
}

$reject = '(?i)unins|setup|update|crash|helper|redist|vcredist|elevate|squirrel|bootstrapper'

# The top-level item under `root` that `full` sits in — the unit a folder
# source is gathered and replaced by (one installed app's folder).
function TopItem($root, $full) {
  $rel = $full.Substring($root.TrimEnd('\').Length).TrimStart('\')
  $first = $rel.Split('\')[0]
  return (Join-Path $root $first)
}

# Every *.exe a folder source covers: the whole root (null) or the listed
# top-level items under it, to the same depth either way.
function FolderExes($root, $items) {
  if ($null -eq $items) {
    return Get-ChildItem -LiteralPath $root -Recurse -Depth 3 -Filter *.exe -File
  }
  $out = @()
  foreach ($it in $items) {
    if (-not (Test-Path -LiteralPath $it)) { continue }
    if (Test-Path -LiteralPath $it -PathType Leaf) {
      if ($it.ToLower().EndsWith('.exe')) { $out += Get-Item -LiteralPath $it }
    } else {
      $out += Get-ChildItem -LiteralPath $it -Recurse -Depth 2 -Filter *.exe -File
    }
  }
  return $out
}

# 4a. Built-in per-user install root — requires a real FileDescription.
$scanRoot = $Params.scanRoot
if ($scanRoot -and (Test-Path -LiteralPath $scanRoot)) {
  foreach ($f in (FolderExes $scanRoot $Params.scan)) {
    if ($f.Name -match $reject) { continue }
    $vi = $f.VersionInfo
    $desc = [string]$vi.FileDescription
    if (-not $desc) { continue }
    [void]$rows.Add([pscustomobject]@{
      origin = 'scan:' + (TopItem $scanRoot $f.FullName)
      name = $desc; exec = $f.FullName; kind = 'exe'
      args = ''; icon = (IconFor $f.FullName); source = 'scan'
      product = [string]$vi.ProductName; company = [string]$vi.CompanyName
      size = [int64]$f.Length
    })
  }
}

# 4b. User-configured extra dirs — lenient: keep metadata-less binaries too
#     (Rust's per-folder "main binary" pick trims the rest). `extra` = null:
#     every extra dir in full; otherwise only the listed top-level items.
foreach ($d in $Params.extraDirs) {
  if (-not $d -or -not (Test-Path -LiteralPath $d)) { continue }
  $items = $null
  if ($null -ne $Params.extra) {
    $items = @($Params.extra | Where-Object { $_.ToLower().StartsWith($d.TrimEnd('\').ToLower() + '\') })
    if ($items.Count -eq 0) { continue }
  }
  foreach ($f in (FolderExes $d $items)) {
    if ($f.Name -match $reject) { continue }
    $vi = $f.VersionInfo
    $desc = [string]$vi.FileDescription
    $nm = if ($desc) { $desc } else { [IO.Path]::GetFileNameWithoutExtension($f.Name) }
    [void]$rows.Add([pscustomobject]@{
      origin = 'extra:' + (TopItem $d $f.FullName)
      name = $nm; exec = $f.FullName; kind = 'exe'
      args = ''; icon = (IconFor $f.FullName); source = 'extra'
      product = [string]$vi.ProductName; company = [string]$vi.CompanyName
      size = [int64]$f.Length
    })
  }
}

# -InputObject (not the pipeline) so a single row still serialises as an array.
ConvertTo-Json -InputObject @($rows) -Compress -Depth 4
