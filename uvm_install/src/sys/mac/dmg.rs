use crate::install::error::InstallerErrorInner::CopyFailed;
use crate::install::error::{InstallerErrorInner, InstallerResult};
use crate::install::installer::{BaseInstaller, Installer, InstallerWithDestination};
use crate::install::{InstallHandler, UnityModule};
use log::debug;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{fs, io};
use thiserror_context::Context;

pub struct Dmg;
pub type ModuleDmgWithDestinationInstaller = Installer<UnityModule, Dmg, InstallerWithDestination>;
pub type ModuleDmgInstaller = Installer<UnityModule, Dmg, BaseInstaller>;

impl<V, I> Installer<V, Dmg, I> {
    // TODO use fs_extra or similar
    // Maybe this is mac specific?
    fn copy_dir<P, D>(&self, source: P, destination: D) -> InstallerResult<()>
    where
        P: AsRef<Path>,
        D: AsRef<Path>,
    {
        let source = source.as_ref();
        let destination = destination.as_ref();

        debug!("Copy {} to {}", source.display(), destination.display());
        let child = Command::new("cp")
            .arg("-a")
            .arg(source)
            .arg(destination)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(CopyFailed(
                source.display().to_string(),
                destination.display().to_string(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            )
            .into());
        }
        Ok(())
    }

    fn copy_dir_contents<P, D>(&self, source: P, destination: D) -> InstallerResult<()>
    where
        P: AsRef<Path>,
        D: AsRef<Path>,
    {
        let source = source.as_ref();
        let destination = destination.as_ref();

        fs::DirBuilder::new().recursive(true).create(destination)?;

        debug!(
            "Copy contents of {} to {}",
            source.display(),
            destination.display()
        );
        let child = Command::new("cp")
            .arg("-a")
            .arg(source.join("."))
            .arg(destination)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(CopyFailed(
                source.display().to_string(),
                destination.display().to_string(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            )
            .into());
        }
        Ok(())
    }

    fn find_file_in_dir<P, F>(&self, dir: P, predicate: F) -> InstallerResult<PathBuf>
    where
        P: AsRef<Path>,
        F: FnMut(&std::fs::DirEntry) -> bool,
    {
        let dir = dir.as_ref();
        debug!("find file in directory {}", dir.display());
        fs::read_dir(dir)
            .and_then(|read_dir| {
                read_dir
                    .filter_map(io::Result::ok)
                    .find(predicate)
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::NotFound,
                            format!("can't locate file in {}", &dir.display()),
                        )
                    })
                    .map(|entry| entry.path())
            })
            .map_err(|err| InstallerErrorInner::IO(err).into())
    }

    fn install_module_from_dmg(&self, dmg_file: &Path, destination: &Path) -> InstallerResult<()> {
        use ::dmg::Attach;

        debug!(
            "install from dmg {} to {}",
            dmg_file.display(),
            destination.display()
        );
        let volume = Attach::new(dmg_file).with()?;
        debug!("installer mounted at {}", volume.mount_point.display());

        let app_path = self
            .find_file_in_dir(&volume.mount_point, |entry| {
                entry.file_name().to_str().unwrap().ends_with(".app")
            })
            .context("failed to find .app in package")?;

        self.copy_dir(app_path, destination)
            .context("failed to copy .app contents to destination")?;
        Ok(())
    }
}

impl InstallHandler for ModuleDmgInstaller {
    fn install_handler(&self) -> InstallerResult<()> {
        let installer = self.installer();
        let destination = Path::new("/Applications");
        self.install_module_from_dmg(installer, destination)
    }

    fn installer(&self) -> &Path {
        self.installer()
    }

    fn after_install(&self) -> InstallerResult<()> {
        if let Some((from, to)) = &self.rename() {
            uvm_move_dir::move_dir(from, to).context("failed to rename installed module")?;
        }
        Ok(())
    }
}

impl InstallHandler for ModuleDmgWithDestinationInstaller {
    fn install_handler(&self) -> InstallerResult<()> {
        let installer = self.installer();
        let destination = self.destination();
        use ::dmg::Attach;

        debug!(
            "install from dmg {} to {}",
            installer.display(),
            destination.display()
        );
        let volume = Attach::new(installer).with()?;
        debug!("installer mounted at {}", volume.mount_point.display());

        let app_path = self
            .find_file_in_dir(&volume.mount_point, |entry| {
                entry.file_name().to_str().unwrap().ends_with(".app")
            })
            .context("failed to find .app in package")?;

        self.copy_dir_contents(app_path, destination)
            .context("failed to copy .app contents to destination")
    }

    fn installer(&self) -> &Path {
        self.installer()
    }

    fn after_install(&self) -> InstallerResult<()> {
        if let Some((from, to)) = &self.rename() {
            uvm_move_dir::move_dir(from, to).context("failed to rename installed module")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn copy_dir_contents_preserves_existing_destination_contents() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("module.app");
        let destination = temp.path().join("destination");
        fs::create_dir_all(source.join("Contents")).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(source.join("Contents/new.txt"), "new").unwrap();
        fs::write(destination.join("existing.txt"), "existing").unwrap();

        let installer = ModuleDmgWithDestinationInstaller::new(
            temp.path().join("module.dmg"),
            &destination,
            None::<(PathBuf, PathBuf)>,
        );
        installer.copy_dir_contents(&source, &destination).unwrap();

        assert_eq!(
            fs::read_to_string(destination.join("existing.txt")).unwrap(),
            "existing"
        );
        assert_eq!(
            fs::read_to_string(destination.join("Contents/new.txt")).unwrap(),
            "new"
        );
    }
}
