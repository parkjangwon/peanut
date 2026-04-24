use std::{fs, path::Path};

fn main() {
    let export_dir = Path::new("peanut-console/out");

    if !export_dir.exists() {
        fs::create_dir_all(export_dir).expect("failed to create peanut-console/out");
    }

    let index_path = export_dir.join("index.html");
    if !index_path.exists() {
        fs::write(
            &index_path,
            r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Peanut Console</title>
  </head>
  <body style="font-family: sans-serif; background: #0a0a0a; color: #ededed; padding: 32px;">
    <h1>Peanut Console</h1>
    <p>Frontend assets are not built yet. Run <code>./scripts/build.sh</code> to embed the exported console.</p>
  </body>
</html>"#,
        )
        .expect("failed to write fallback peanut console asset");
    }

    println!("cargo:rerun-if-changed=peanut-console/src");
    println!("cargo:rerun-if-changed=peanut-console/public");
    println!("cargo:rerun-if-changed=peanut-console/next.config.mjs");
}
