use std::{env, fs, path::PathBuf};

fn main() {
    let commit = env::var("CMRAK_BUILD_COMMIT").ok().filter(|v| !v.is_empty()).or_else(|| {
        std::process::Command::new("git").args(["rev-parse", "HEAD"]).output().ok()
            .filter(|o| o.status.success()).and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_owned())
    }).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=CMRAK_BUILD_COMMIT={commit}");
    println!("cargo:rerun-if-changed=generated");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let generated = root.join("generated");
    assert!(
        generated.join("render.js").exists(),
        "Run `npm run build:cmrak-assets` before building cmrak"
    );
    let mut assets = Vec::new();
    for directory in [generated.clone(), generated.join("fonts")] {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file()
                && path.file_name().unwrap() != "render.js"
                && path.file_name().unwrap() != "intl.js"
                && path.file_name().unwrap() != "version.txt"
            {
                assets.push(path);
            }
        }
    }
    assets.sort();
    let mut source = String::from("pub const ASSETS: &[(&str, &[u8])] = &[\n");
    for path in assets {
        let name = path
            .strip_prefix(&generated)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        source.push_str(&format!(
            "({name:?}, include_bytes!({:?})),\n",
            path.to_str().unwrap()
        ));
    }
    source.push_str("];\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("assets.rs"),
        source,
    )
    .unwrap();
}
