use crate::install::error::InstallerResult;
use crate::install::installer::{Installer, InstallerWithDestination};
use crate::install::{InstallHandler, UnityModule};
use crate::*;
use ::zip;
use std::fs::File;
use thiserror_context::Context;

pub struct Zip;
pub type ModuleZipInstaller = Installer<UnityModule, Zip, InstallerWithDestination>;

impl<V, I> Installer<V, Zip, I> {
    #[allow(dead_code)]
    pub fn deploy_zip(&self, installer: &Path, destination: &Path) -> InstallerResult<()> {
        self.deploy_zip_with_rename(installer, destination, |p| p.to_path_buf())
    }

    fn deploy_zip_with_rename<F>(
        &self,
        installer: &Path,
        destination: &Path,
        rename_handler: F,
    ) -> InstallerResult<()>
    where
        F: Fn(&Path) -> PathBuf,
    {
        let file = File::open(installer).context("failed to open zip file")?;
        let mut archive = zip::ZipArchive::new(file)?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).expect("expect file entry at index 0");
            let output_path = rename_handler(&destination.join(file.mangled_name()));
            {
                let comment = file.comment();
                if !comment.is_empty() {
                    trace!("File {} comment: {}", i, comment);
                }
            }

            if (&*file.name()).ends_with('/') {
                debug!(
                    "File {} extracted to \"{}\"",
                    i,
                    output_path.as_path().display()
                );
                fs::DirBuilder::new()
                    .recursive(true)
                    .create(&output_path)
                    .context(format!(
                        "failed to create output path {}",
                        output_path.display()
                    ))?;
            } else {
                debug!(
                    "File {} extracted to \"{}\" ({} bytes)",
                    i,
                    output_path.as_path().display(),
                    file.size()
                );
                if let Some(p) = output_path.parent() {
                    if !p.exists() {
                        fs::DirBuilder::new()
                            .recursive(true)
                            .create(&p)
                            .context(format!(
                                "failed to create parent directory {} for output path {}",
                                p.display(),
                                output_path.display()
                            ))?;
                    }
                }
                let mut outfile = fs::File::create(&output_path)?;
                io::copy(&mut file, &mut outfile).context(format!(
                    "failed to copy file {} to output path {}",
                    file.name(),
                    output_path.display()
                ))?;
            }

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = file.unix_mode() {
                    fs::set_permissions(&output_path, fs::Permissions::from_mode(mode)).context(
                        format!(
                            "failed to set permissions on file {}",
                            output_path.display()
                        ),
                    )?;
                }
            }
        }

        Ok(())
    }
}

impl InstallHandler for ModuleZipInstaller {
    fn install_handler(&self) -> InstallerResult<()> {
        let rename = self.rename();

        let rename_handler = |path: &Path| match rename {
            Some((from, to)) => path.strip_prefix(from).map(|p| to.join(p)).unwrap(),
            None => path.to_path_buf(),
        };

        let installer = self.installer();
        let destination = self.destination();

        debug!(
            "install module from zip archive {} to {}",
            installer.display(),
            destination.display()
        );

        self.deploy_zip_with_rename(installer, destination, rename_handler)
    }

    fn installer(&self) -> &Path {
        self.installer()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;

    #[test]
    fn module_install_preserves_existing_destination_contents() {
        let temp = TempDir::new().unwrap();
        let archive_path = temp.path().join("module.zip");
        let destination = temp.path().join("destination");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("existing.txt"), "existing").unwrap();

        let archive_file = File::create(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(archive_file);
        archive
            .start_file("new/new.txt", SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"new").unwrap();
        archive.finish().unwrap();

        let installer = ModuleZipInstaller::new(
            &archive_path,
            &destination,
            None::<(PathBuf, PathBuf)>,
        );
        installer.install().unwrap();

        assert_eq!(
            fs::read_to_string(destination.join("existing.txt")).unwrap(),
            "existing"
        );
        assert_eq!(
            fs::read_to_string(destination.join("new/new.txt")).unwrap(),
            "new"
        );
    }
}
