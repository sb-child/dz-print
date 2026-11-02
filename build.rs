fn main() {
    compile_dzserver_protos();
}

#[cfg(feature = "bin-dzserver")]
fn compile_dzserver_protos() {
    // 你可能需要 sudo dnf install protobuf-compiler
    tonic_prost_build::compile_protos("proto/dzsrv.proto")
        .unwrap_or_else(|e| panic!("Failed to compile protos {:?}", e));
}
#[cfg(not(feature = "bin-dzserver"))]
fn compile_dzserver_protos() {}
