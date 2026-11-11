//! Private, atomic image output. Existing destinations are never opened for writing.
use crate::Result;
use image::{GrayImage, ImageFormat};
use std::path::Path;

/// Save an image through a private same-directory temporary file. A final-path
/// symlink is rejected; a symlink raced into place is replaced, never followed.
/// Existing regular output is atomically replaced, including its old permissions.
/// Parent directories must be trusted. Unix files are created with mode 0600;
/// other platforms inherit the directory's access controls.
pub fn save_gray(image: &GrayImage, path: &Path) -> Result<()> {
    let format = ImageFormat::from_path(path)?;
    match std::fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_file() => {
            return Err(crate::Error::msg(
                "image output must not be a symlink or special file",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::Builder::new()
        .prefix(".thermark-image-")
        .tempfile_in(parent)?;
    image.write_to(temp.as_file_mut(), format)?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .map_err(|error| crate::Error::from(error.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creates_and_replaces_readable_image() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("label.png");
        save_gray(&GrayImage::new(2, 3), &path).unwrap();
        save_gray(&GrayImage::new(3, 2), &path).unwrap();
        assert_eq!(image::image_dimensions(&path).unwrap(), (3, 2));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[cfg(unix)]
    #[test]
    fn refuses_links_and_makes_replacements_private() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, b"keep").unwrap();
        let path = dir.path().join("label.png");
        symlink(&target, &path).unwrap();
        assert!(save_gray(&GrayImage::new(1, 1), &path).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
        std::fs::remove_file(&path).unwrap();
        std::fs::hard_link(&target, &path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        save_gray(&GrayImage::new(1, 1), &path).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    #[test]
    fn failed_encoding_keeps_existing_output() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("label.unsupported");
        std::fs::write(&path, b"keep").unwrap();
        assert!(save_gray(&GrayImage::new(1, 1), &path).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
