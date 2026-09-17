fn main() {
    tauri_build::build();

    #[cfg(target_os = "windows")]
    embed_windows_manifest();
}

#[cfg(target_os = "windows")]
fn embed_windows_manifest() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let manifest_path = std::path::Path::new(&out_dir).join("comctl6.manifest");
    let rc_path = std::path::Path::new(&out_dir).join("comctl6.rc");
    let res_path = std::path::Path::new(&out_dir).join("comctl6.o");

    let manifest = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
</assembly>"#;

    if std::fs::write(&manifest_path, manifest).is_ok() {
        let rc_content = format!("1 24 \"{}\"", manifest_path.display().to_string().replace('\\', "/"));
        if std::fs::write(&rc_path, rc_content).is_ok() {
            let status = std::process::Command::new("windres")
                .args(&[
                    rc_path.to_str().unwrap(),
                    "-O",
                    "coff",
                    "-o",
                    res_path.to_str().unwrap(),
                ])
                .status();
            if let Ok(s) = status {
                if s.success() {
                    println!("cargo:rustc-link-arg={}", res_path.display());
                }
            }
        }
    }
}
