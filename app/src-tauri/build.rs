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
    tauri_build::build()
}
