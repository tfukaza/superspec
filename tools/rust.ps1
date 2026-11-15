param([ValidateSet('test','build','wasm','wasm-threads','wasm-tests','check','fmt')][string]$Task = 'test')
$ErrorActionPreference = 'Stop'
$originalRustFlags = $env:RUSTFLAGS
$originalBootstrap = $env:RUSTC_BOOTSTRAP
Push-Location (Join-Path $PSScriptRoot '..')
try {
    $rustLocal = Join-Path (Get-Location).Path 'generated/rust-toolchain'
    $cargoCommand = 'cargo'
    $bindgenCommand = 'wasm-bindgen'
    if (Test-Path "$rustLocal/cargo/bin/cargo.exe") {
        $env:CARGO_HOME = "$rustLocal/cargo"
        $env:RUSTUP_HOME = "$rustLocal/rustup"
        $cargoCommand = "$rustLocal/cargo/bin/cargo.exe"
        $llvmBin = "$rustLocal/llvm-mingw/llvm-mingw-20261104-msvcrt-x86_64/bin"
        if (Test-Path $llvmBin) {
            $env:PATH = "$llvmBin;$env:PATH"
            $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = "$rustLocal/rustup/toolchains/1.100.0-x86_64-pc-windows-gnu/lib/rustlib/x86_64-pc-windows-gnu/bin/self-contained/x86_64-w64-mingw32-gcc.exe"
        }
        $localBindgen = "$rustLocal/wasm-bindgen-0.2.130-x86_64-pc-windows-msvc/wasm-bindgen.exe"
        if (Test-Path $localBindgen) { $bindgenCommand = $localBindgen }
    }
    switch ($Task) {
        'test' { & $cargoCommand test --workspace --locked }
        'build' { & $cargoCommand build -p superspec-cli --release --locked }
        'check' { & $cargoCommand check --workspace --all-targets --locked }
        'fmt' { & $cargoCommand fmt --all }
        'wasm' {
            & $cargoCommand build -p superspec-wasm --target wasm32-unknown-unknown --release --locked
            if ($LASTEXITCODE -ne 0) { throw 'Wasm compilation failed.' }
            & $bindgenCommand --target web --out-dir generated/wasm target/wasm32-unknown-unknown/release/superspec_wasm.wasm
        }
        'wasm-threads' {
            $env:RUSTC_BOOTSTRAP = '1'
            $env:RUSTFLAGS = '-C target-feature=+atomics,+bulk-memory -C link-arg=--shared-memory -C link-arg=--max-memory=4294967296 -C link-arg=--import-memory -C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size -C link-arg=--export=__tls_align -C link-arg=--export=__tls_base'
            & $cargoCommand build -p superspec-wasm --target wasm32-unknown-unknown --release --features threads -Z build-std=panic_abort,std --target-dir generated/rust-thread-target
            if ($LASTEXITCODE -ne 0) { throw 'Threaded Wasm compilation failed.' }
            & $bindgenCommand --target web --out-dir generated/wasm-threaded generated/rust-thread-target/wasm32-unknown-unknown/release/superspec_wasm.wasm
            [IO.File]::WriteAllText((Join-Path (Get-Location) 'generated/wasm-threaded/package.json'), '{"name":"superspec-wasm-threaded","type":"module","main":"superspec_wasm.js"}')
        }
        'wasm-tests' {
            & $cargoCommand build -p superspec-kernel-acceptance --target wasm32-unknown-unknown --release --locked
            if ($LASTEXITCODE -ne 0) { throw 'Wasm acceptance compilation failed.' }
            & $bindgenCommand --target web --out-dir generated/wasm-tests target/wasm32-unknown-unknown/release/superspec_kernel_acceptance.wasm
        }
    }
    if ($LASTEXITCODE -ne 0) { throw "Rust $Task failed with exit code $LASTEXITCODE." }
} finally {
    $env:RUSTFLAGS = $originalRustFlags
    $env:RUSTC_BOOTSTRAP = $originalBootstrap
    Pop-Location
}
