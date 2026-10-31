use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=generated");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let generated = root.join("generated");
    assert!(
        generated.join("render.js").exists(),
        "Run `npm run build:comrak-assets` before building lib-comrak"
    );
    let mut assets = Vec::new();
    for directory in [generated.clone(), generated.join("fonts")] {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file()
                && path.file_name().unwrap() != "render.js"
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
