use std::fs;

fn main() {
    let folder = "runtime/llama";
    let _ = fs::create_dir_all(folder);
    if fs::read_dir(folder).map_or(true, |mut files| files.next().is_none()) {
        let _ = fs::write(
            format!("{folder}/NOT-BUILT.txt"),
            "The embedding program hasn't been built. Run `npm run build:runtime` in app/.\n",
        );
    }
    println!("cargo:rerun-if-changed={folder}");
    let notices = "runtime/notices.json";
    let missing = "{\"missing\":true,\"platform\":\"\",\"texts\":[],\"sections\":[]}";
    if !std::path::Path::new(notices).exists() {
        let _ = fs::write(notices, missing);
    }
    if std::env::var("PROFILE").as_deref() == Ok("release")
        && fs::read_to_string(notices).is_ok_and(|json| json == missing)
    {
        panic!(
            "The licenses haven't been gathered: run `npm run build:runtime` (or `node scripts/build-notices.mjs`) in app/"
        );
    }
    println!("cargo:rerun-if-changed={notices}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let folder = "runtime/ocr";
        let _ = fs::create_dir_all(folder);
        if fs::read_dir(folder).map_or(true, |mut files| files.next().is_none()) {
            let _ = fs::write(
                format!("{folder}/NOT-COPIED.txt"),
                "The OCR's DLLs are copied here by `node scripts/copy-ocr-runtime.mjs` in app/, which `tauri build` runs.\n",
            );
        }
        println!("cargo:rerun-if-changed={folder}");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib/Mesa/onnxruntime:$ORIGIN/onnxruntime:$ORIGIN");
    }
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
