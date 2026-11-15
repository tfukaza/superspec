param(
    [string]$Output = 'generated/native-release',
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location $repository
try {
    if (-not $SkipBuild) { & (Join-Path $PSScriptRoot 'rust.ps1') build }
    $windowsHost = [Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT
    $executableNames = @('superspec', 'superspec-studio', 'superspec-materials') | ForEach-Object { if ($windowsHost) { "$_.exe" } else { $_ } }
    $reviewer = Join-Path $repository 'generated/rust-editor'
    foreach ($name in $executableNames) {
        if (-not (Test-Path -LiteralPath (Join-Path $repository "target/release/$name") -PathType Leaf)) { throw "Build the native release executable $name first." }
    }
    foreach ($page in @('index.html', 'preview.html', 'material.html', 'item.html')) {
        if (-not (Test-Path -LiteralPath (Join-Path $reviewer $page) -PathType Leaf)) { throw "Build the browser Wasm and $page assets first." }
    }
    $destination = if ([IO.Path]::IsPathRooted($Output)) { [IO.Path]::GetFullPath($Output) } else { [IO.Path]::GetFullPath((Join-Path $repository $Output)) }
    if (Test-Path -LiteralPath $destination) { throw "Release destination already exists: $destination" }
    New-Item -ItemType Directory -Path $destination | Out-Null
    foreach ($name in $executableNames) {
        Copy-Item -LiteralPath (Join-Path $repository "target/release/$name") -Destination (Join-Path $destination $name)
    }
    Copy-Item -LiteralPath $reviewer -Destination (Join-Path $destination 'reviewer') -Recurse
    Copy-Item -LiteralPath (Join-Path $repository 'assets/materials') -Destination (Join-Path $destination 'materials') -Recurse
    Copy-Item -LiteralPath (Join-Path $repository 'LICENSE') -Destination (Join-Path $destination 'LICENSE')
    @'
SuperSpec native preview and compiler

Run ./superspec --help (superspec.exe on Windows).
Run ./superspec init my-project, then ./superspec preview my-project/level.aml.
Run ./superspec-studio --help for material, item and saved-world source checks and exports.
The preview server also serves /material.html and /item.html for browser authoring.
The packaged reviewer and materials directories belong beside the executable.
The preview host and all model compilation run in Rust. A browser displays the reviewer.
Blender and Quake tools are optional external programs for their corresponding export commands.
'@ | Set-Content -LiteralPath (Join-Path $destination 'README.txt') -Encoding utf8
    Write-Output $destination
} finally {
    Pop-Location
}
