fn main() {
    #[cfg(target_os = "windows")]
    {
        windows::build();
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::fs::File;
    use std::io::Write;

    extern crate embed_resource;

    pub fn build() {
        // let version: String = env!("CARGO_PKG_VERSION").to_string();
        // let win_version = {
        //     let mut ver_vec = version.split(".").collect::<Vec<_>>();
        //     ver_vec.push("0");
        //     ver_vec.join(",")
        // };

        let rc_path = "./target/kanata_ime_observer.exe.manifest.rc";
        let manifest_path = "./target/kanata_ime_observer.exe.manifest";

        let rc_str = format!(
            r#"
#define RT_MANIFEST 24
1 RT_MANIFEST "{manifest_path}"
        "#
        );

        let mut rc_file = File::create(rc_path).unwrap();
        rc_file.write_all(rc_str.as_bytes()).unwrap();

        let manifest_str = r#"
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
</assembly>
"#
        .to_string();

        let mut manifest_file = File::create(manifest_path).unwrap();
        manifest_file.write_all(manifest_str.as_bytes()).unwrap();

        embed_resource::compile(rc_path, embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}
