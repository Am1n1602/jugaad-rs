fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Uses a vendored `protoc` binary rather than requiring one on PATH -
    // keeps `cargo build` working with no system setup, matching every
    // other crate in this workspace. `Config::protoc_executable` (rather
    // than the `PROTOC` env var) avoids needing `std::env::set_var`, which
    // is `unsafe` as of the 2024 edition and this workspace forbids
    // unsafe code entirely.
    let mut config = tonic_prost_build::Config::new();
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);

    tonic_prost_build::configure().compile_with_config(
        config,
        &["proto/jugaad.proto"],
        &["proto"],
    )?;
    Ok(())
}
