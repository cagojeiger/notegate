use std::env;
use std::fs::File;
use std::io::{self, Cursor};
use std::path::PathBuf;

use utoipa_swagger_ui_vendored::SWAGGER_UI_VENDORED;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");

    let output_dir = PathBuf::from(env::var("OUT_DIR")?);
    let mut archive = zip::ZipArchive::new(Cursor::new(SWAGGER_UI_VENDORED))?;
    for (archive_path, output_name) in [
        ("swagger-ui-5.32.6/dist/index.html", "index.html"),
        ("swagger-ui-5.32.6/dist/index.css", "index.css"),
        ("swagger-ui-5.32.6/dist/swagger-ui.css", "swagger-ui.css"),
        (
            "swagger-ui-5.32.6/dist/swagger-ui-bundle.js",
            "swagger-ui-bundle.js",
        ),
        (
            "swagger-ui-5.32.6/dist/swagger-ui-standalone-preset.js",
            "swagger-ui-standalone-preset.js",
        ),
        (
            "swagger-ui-5.32.6/dist/favicon-16x16.png",
            "favicon-16x16.png",
        ),
        (
            "swagger-ui-5.32.6/dist/favicon-32x32.png",
            "favicon-32x32.png",
        ),
        ("swagger-ui-5.32.6/LICENSE", "LICENSE"),
        ("swagger-ui-5.32.6/NOTICE", "NOTICE"),
    ] {
        let mut source = archive.by_name(archive_path)?;
        let mut target = File::create(output_dir.join(output_name))?;
        io::copy(&mut source, &mut target)?;
    }
    Ok(())
}
