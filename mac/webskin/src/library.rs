//! The skin library: the skins kept in the support folder's Skins folder, which the Skins menu lists.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// ~/Library/Application Support/WebSkin Player/Skins (under $WEBSKIN_HOME in tests and the corpus runner).
pub fn folder() -> PathBuf {
    crate::settings::support_dir().join("Skins")
}

fn is_skin(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.ends_with(".wmz") || lower.ends_with(".wms")
}

/// The skins in `dir`, in name order.
pub fn list(dir: &Path) -> Vec<PathBuf> {
    let mut skins: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(is_skin))
        .collect();
    skins.sort_by_key(|p| name(p).to_lowercase());
    skins
}

/// What the Skins menu calls a skin: its file name without the extension.
pub fn name(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Copies the skins in `source` that `library` doesn't have yet: one skin, a folder of them, or a .zip of them.
/// Returns how many were added.
pub fn import(source: &Path, library: &Path) -> io::Result<usize> {
    fs::create_dir_all(library)?;
    let mut added = 0;
    if source.is_dir() {
        for skin in list(source) {
            added += usize::from(copy_new(&skin, library)?);
        }
    } else if source.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
        let mut zip = zip::ZipArchive::new(fs::File::open(source)?).map_err(io::Error::other)?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(io::Error::other)?;
            let Some(file) = entry.enclosed_name().and_then(|p| p.file_name().map(PathBuf::from)) else { continue };
            if !entry.is_file() || !file.to_str().is_some_and(is_skin) || library.join(&file).exists() {
                continue;
            }
            io::copy(&mut entry, &mut fs::File::create(library.join(&file))?)?;
            added += 1;
        }
    } else if source.file_name().and_then(|n| n.to_str()).is_some_and(is_skin) {
        added += usize::from(copy_new(source, library)?);
    }
    Ok(added)
}

/// Copies one skin unless the library has one by that name.
fn copy_new(skin: &Path, library: &Path) -> io::Result<bool> {
    let Some(file) = skin.file_name() else { return Ok(false) };
    let target = library.join(file);
    if target.exists() {
        return Ok(false);
    }
    fs::copy(skin, target)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("webskin-library-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lists_skins_by_name() {
        let dir = scratch("list");
        for f in ["zeta.wmz", "Alpha.WMZ", "beta.wms", "notes.txt"] {
            fs::write(dir.join(f), b"x").unwrap();
        }
        let names: Vec<String> = list(&dir).iter().map(|p| name(p.as_path())).collect();
        assert_eq!(names, ["Alpha", "beta", "zeta"]);
    }

    #[test]
    fn imports_a_folder_once() {
        let (from, to) = (scratch("folder-from"), scratch("folder-to"));
        fs::write(from.join("one.wmz"), b"1").unwrap();
        fs::write(from.join("two.wmz"), b"2").unwrap();
        fs::write(from.join("readme.txt"), b"-").unwrap();
        fs::write(to.join("two.wmz"), b"mine").unwrap();
        assert_eq!(import(&from, &to).unwrap(), 1);
        assert_eq!(fs::read(to.join("two.wmz")).unwrap(), b"mine");
        assert_eq!(import(&from, &to).unwrap(), 0);
        assert_eq!(import(&from.join("one.wmz"), &scratch("single")).unwrap(), 1);
    }

    #[test]
    fn imports_the_skins_in_a_zip() {
        let (dir, to) = (scratch("zip"), scratch("zip-to"));
        let mut data = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut data));
            for entry in ["Skins/heart.wmz", "Skins/Goo.wmz", "Read Me.txt"] {
                w.start_file(entry, zip::write::SimpleFileOptions::default()).unwrap();
                w.write_all(entry.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        fs::write(dir.join("skins.zip"), &data).unwrap();
        assert_eq!(import(&dir.join("skins.zip"), &to).unwrap(), 2);
        assert_eq!(fs::read(to.join("heart.wmz")).unwrap(), b"Skins/heart.wmz");
        assert_eq!(list(&to).len(), 2);
    }
}
