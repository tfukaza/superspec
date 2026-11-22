param(
    [string]$Output = 'generated/native-release',
    [switch]$SkipBuild,
    [switch]$NativeOnly
)
$ErrorActionPreference = 'Stop'
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location $repository
try {
    if (-not $SkipBuild) { & (Join-Path $PSScriptRoot 'rust.ps1') build
        npm run build:native-viewer
        if ($LASTEXITCODE -ne 0) { throw 'Native viewer build failed.' }
    }
    $windowsHost = [Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT
    $executableNames = @('superspec', 'superspec-studio', 'superspec-materials', 'superspec-gi') | ForEach-Object { if ($windowsHost) { "$_.exe" } else { $_ } }
    $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
    $release = Join-Path $targetDir 'release'
    $nativeViewer = Join-Path $repository 'generated/native-viewer'
    $reviewer = Join-Path $repository 'generated/rust-editor'
    foreach ($name in $executableNames) {
        if (-not (Test-Path -LiteralPath (Join-Path $release $name) -PathType Leaf)) { throw "Build the native release executable $name first." }
    }
    foreach ($page in @('index.html', 'preview.html', 'material.html', 'item.html', 'native.html')) {
        if (-not (Test-Path -LiteralPath (Join-Path $nativeViewer $page) -PathType Leaf)) { throw "Build the native viewer and $page assets first." }
    }
    if (-not $NativeOnly -and -not (Test-Path -LiteralPath (Join-Path $reviewer 'index.html') -PathType Leaf)) {
        throw 'Build the reference reviewer first, or pass -NativeOnly to package native Studio alone.'
    }
    $destination = if ([IO.Path]::IsPathRooted($Output)) { [IO.Path]::GetFullPath($Output) } else { [IO.Path]::GetFullPath((Join-Path $repository $Output)) }
    if (Test-Path -LiteralPath $destination) { throw "Release destination already exists: $destination" }
    New-Item -ItemType Directory -Path $destination | Out-Null
    foreach ($name in $executableNames) {
        Copy-Item -LiteralPath (Join-Path $release $name) -Destination (Join-Path $destination $name)
    }
    Copy-Item -LiteralPath $nativeViewer -Destination (Join-Path $destination 'native-viewer') -Recurse
    if (-not $NativeOnly) { Copy-Item -LiteralPath $reviewer -Destination (Join-Path $destination 'reviewer') -Recurse }
    Copy-Item -LiteralPath (Join-Path $repository 'assets/materials') -Destination (Join-Path $destination 'materials') -Recurse
    Copy-Item -LiteralPath (Join-Path $repository 'LICENSE') -Destination (Join-Path $destination 'LICENSE')
    @'
SuperSpec native preview and compiler

Run ./superspec --help (superspec.exe on Windows).
Run ./superspec-studio serve and open the printed address for native authoring.
With the optional reviewer package, ./superspec preview my-project/level.aml opens the reference preview.
Run ./superspec-studio --help for material, item and saved-world source checks and exports.
superspec-gi reads a Z-up colored-triangle probe request on stdin and emits six RGB8 irradiance volumes.
The preview server also serves /material.html and /item.html for browser authoring.
The packaged reviewer and materials directories belong beside the executable.
The preview host and all model compilation run in Rust. A browser displays the reviewer.
Blender and Quake tools are optional external programs for their corresponding export commands.
'@ | Set-Content -LiteralPath (Join-Path $destination 'README.txt') -Encoding utf8
    Write-Output $destination
} finally {
    Pop-Location
}
