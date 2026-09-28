fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc not found");
    unsafe { std::env::set_var("PROTOC", protoc) };

    let mut config = prost_build::Config::new();
    config.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]");
    config.type_attribute(".", "#[allow(dead_code)]");
    config.type_attribute(".notepad.", "#[serde(rename_all = \"camelCase\")]");
    config
        .compile_protos(&["proto/notepad.proto"], &["proto/"])
        .expect("prost codegen failed");
}
