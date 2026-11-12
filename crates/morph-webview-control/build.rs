fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    // SAFETY: build.rs is single-threaded; tonic-build reads PROTOC during compile_protos.
    unsafe {
        std::env::set_var("PROTOC", &protoc);
    }
    println!("cargo:rerun-if-changed=proto/morph.webview.v1.proto");
    tonic_build::configure().build_server(true).build_client(true).compile_protos(&["proto/morph.webview.v1.proto"], &["proto"])?;
    Ok(())
}
