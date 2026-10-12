fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            // 独立可执行文件无需导入库和导出文件，避免链接器输出无用的生成消息。
            println!("cargo:rustc-link-arg-bin=pedal=/NOIMPLIB");
            println!("cargo:rustc-link-arg-bin=pedal=/NOEXP");
        }
        let mut res = winresource::WindowsResource::new();
        res.set_icon("icon.ico");
        res.compile().expect("failed to compile windows resources");
    }
}
