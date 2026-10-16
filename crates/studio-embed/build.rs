//! Puts the Studio's build output where the crate can embed it.
//!
//! The SPA is built by `studio/` (bun + `adapter-static`) and its output is not
//! committed, so something has to place it before `rust-embed` reads the folder.
//! Three sources are tried, in order:
//!
//! 1. `LAZYPOCK_STUDIO_DIR`, which CI and the Dockerfile set to a bundle that
//!    has already been built. This is what makes the release image's Studio
//!    exactly the bundle CI built and tested.
//! 2. `studio/build/studio`, the SvelteKit output inside this repository.
//! 3. Nothing — `dist/` is left empty, so a machine without bun can still
//!    build the server. A build with no Studio is a legitimate configuration
//!    (a headless API), so it warns rather than failing; the server then says
//!    so at `/_/` instead of serving a broken page.

use std::path::{Path, PathBuf};

/// The file whose presence means "this is a built Studio bundle".
const ENTRY: &str = "index.html";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=LAZYPOCK_STUDIO_DIR");

    let manifest = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"),
    );
    let dist = manifest.join("dist");

    // `crates/studio-embed` -> the repository root.
    let repository = manifest
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf);

    let explicit = std::env::var_os("LAZYPOCK_STUDIO_DIR").map(PathBuf::from);
    let from_repository = repository.map(|root| root.join("studio/build/studio"));

    let source = explicit
        .as_ref()
        .filter(|dir| is_bundle(dir))
        .or_else(|| from_repository.as_ref().filter(|dir| is_bundle(dir)));

    match source {
        Some(source) if source != &dist => {
            println!("cargo:rerun-if-changed={}", source.display());
            copy_bundle(source, &dist);
        }
        // `dist` is itself the source: a bundle placed there by hand or by CI
        // is already what we want to embed.
        Some(_) => {}
        None => {
            if !is_bundle(&dist) {
                // `rust-embed` needs the folder to exist even when it holds
                // nothing, or the crate fails to compile.
                if let Err(err) = std::fs::create_dir_all(&dist) {
                    panic!("cannot create {}: {err}", dist.display());
                }
                println!(
                    "cargo:warning=lazypock: no Studio bundle found, so this build serves the \
                     API only. Set LAZYPOCK_STUDIO_DIR to a built bundle, or run `bun run build` \
                     in studio/, then rebuild."
                );
            }
        }
    }
}

/// A bundle is a directory holding the SPA's entry point.
fn is_bundle(dir: &Path) -> bool {
    dir.join(ENTRY).is_file()
}

/// Replace `dist` with a copy of `source`.
///
/// Skipped when the copy already looks current, because writing into the source
/// tree on every build would make `rust-embed` re-embed and rebuild the crate
/// each time.
fn copy_bundle(source: &Path, dist: &Path) {
    if is_current(source, dist) {
        return;
    }

    if dist.exists() {
        if let Err(err) = std::fs::remove_dir_all(dist) {
            panic!("cannot clear {}: {err}", dist.display());
        }
    }
    if let Err(err) = copy_tree(source, dist) {
        panic!(
            "cannot copy the Studio bundle into {}: {err}",
            dist.display()
        );
    }
}

/// Whether `dist` already holds this bundle.
///
/// `index.html` is the marker because the SvelteKit build hashes every asset
/// name into it, so any change to the bundle changes this file too.
fn is_current(source: &Path, dist: &Path) -> bool {
    let (from, to) = (source.join(ENTRY), dist.join(ENTRY));
    let (Ok(from), Ok(to)) = (from.metadata(), to.metadata()) else {
        return false;
    };
    match (from.modified(), to.modified()) {
        (Ok(from), Ok(to)) => from <= to,
        // Without usable timestamps, copy: correctness over a rebuild.
        _ => false,
    }
}

/// Copy a directory tree.
fn copy_tree(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
