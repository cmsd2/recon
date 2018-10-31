cargo build

$old_rust_log=$env:RUST_LOG

$env:RUST_LOG="debug"

& ../target/debug/hello-actix.exe

$env:RUST_LOG=$old_rust_log