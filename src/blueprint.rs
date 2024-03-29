use std::borrow::Cow;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use cap_std_ext::cap_std::fs::{Dir, Permissions, PermissionsExt};
use cap_std_ext::dirext::CapStdExtDirExt;
use fn_error_context::context;
use serde::{Deserialize, Serialize};

use crate::{Render, Rendered};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Blueprint {
    pub(crate) packages: Option<Packages>,
    pub(crate) customizations: Option<Customizations>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Customizations {
    pub(crate) sshkey: Option<Vec<Sshkey>>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Packages(Vec<Package>);

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Package {
    pub(crate) name: String,
    pub(crate) version: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Sshkey {
    pub(crate) user: String,
    #[serde(rename = "key")]
    pub(crate) pubkey: String,
}

impl Render for Sshkey {
    fn render(&self, root: &Dir, out: &mut Rendered) -> Result<bool> {
        out.filesystem.create_dir_all(super::USR_TMPFILES)?;
        let tmpfiles = out.filesystem.open_dir(super::USR_TMPFILES)?;
        if self.user != "root" {
            anyhow::bail!("Configuring ssh key for non-root user is not currently supported");
        }

        // Eagerly resolve the path of /root in order to avoid tmpfiles.d clashes/problems.
        // If it's local state (i.e. /root -> /var/roothome) then we resolve that symlink now.
        let roothome_meta = root.symlink_metadata_optional("root")?;
        let root_path = if roothome_meta.as_ref().filter(|m| m.is_symlink()).is_some() {
            let path = root.read_link("root")?;
            Utf8PathBuf::try_from(path)
                .context("Reading /root symlink")
                .map(Cow::Owned)?
        } else {
            Cow::Borrowed(Utf8Path::new("root"))
        };

        let encoded = data_encoding::BASE64.encode(self.pubkey.as_bytes());
        let tmpfiles_content =
            format!("f~ /{root_path}/.ssh/authorized_keys 600 root root - {encoded}\n");
        tmpfiles.atomic_write_with_perms(
            &format!("{}-root-ssh.conf", clap::crate_name!()),
            &tmpfiles_content,
            Permissions::from_mode(0o600),
        )?;
        Ok(true)
    }
}

impl Render for Packages {
    fn render(&self, _root: &Dir, out: &mut Rendered) -> Result<bool> {
        let packages = self.0.iter().map(|pkg| {
            if let Some(v) = pkg.version.as_deref() {
                format!("{}-{}", pkg.name, v)
            } else {
                pkg.name.clone()
            }
        });
        let cmd = ["dnf", "install", "-y"]
            .into_iter()
            .map(ToOwned::to_owned)
            .chain(packages)
            .collect::<Vec<_>>();
        out.exec.push(crate::ExecuteCommand(cmd));
        Ok(true)
    }
}

impl Render for Blueprint {
    #[context("Rendering blueprint")]
    fn render(&self, root: &Dir, out: &mut Rendered) -> Result<bool> {
        let mut changed = false;
        if let Some(customizations) = self.customizations.as_ref() {
            if let Some(keys) = customizations.sshkey.as_ref() {
                for key in keys {
                    key.render(root, out)?;
                    changed = true;
                }
            }
        }

        if let Some(packages) = self.packages.as_ref() {
            if packages.render(root, out)? {
                changed = true;
            }
        }

        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cap_std_ext::cap_std;
    use indoc::indoc;

    #[test]
    fn test_empty() -> Result<()> {
        let root = &cap_std_ext::cap_tempfile::TempDir::new(cap_std::ambient_authority())?;

        let empty: Blueprint = toml::from_str("[customizations]")?;
        let mut r = Rendered::new()?;
        let changed = empty.render(root, &mut r).unwrap();
        assert!(r.exec.is_empty());
        assert_eq!(r.filesystem.entries()?.count(), 0);
        assert!(!changed);
        Ok(())
    }

    #[test]
    fn test_invalid() -> Result<()> {
        let root = &cap_std_ext::cap_tempfile::TempDir::new(cap_std::ambient_authority())?;
        for case in ["", "[foo]\nbar=baz"] {
            let empty: Blueprint = toml::from_str(case)?;
            let mut r = Rendered::new()?;
            assert!(empty.render(root, &mut r).is_err());
            assert_eq!(root.entries()?.count(), 0);
        }
        Ok(())
    }

    #[test]
    fn test_sshkeys() -> Result<()> {
        let root = &cap_std_ext::cap_tempfile::TempDir::new(cap_std::ambient_authority())?;

        // Empty keys

        let blueprint: Blueprint = toml::from_str(indoc! { r#"
            [[customizations.sshkey]]
            user = "root"
            key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA1hVwAoNDa64+woHYxkHfdu6qmdC1BsLXReeQz/CBri example@demo"
            "#
        })?;
        let mut r = Rendered::new()?;
        let changed = blueprint.render(root, &mut r).unwrap();
        // Nothing to execute for this
        assert!(r.exec.is_empty());
        assert_eq!(r.filesystem.entries()?.count(), 1);
        assert!(changed);
        let filename = format!(
            "{}/{}-root-ssh.conf",
            crate::USR_TMPFILES,
            clap::crate_name!()
        );
        let contents = r.filesystem.read_to_string(&filename)?;
        assert_eq!(contents, "f~ /root/.ssh/authorized_keys 600 root root - c3NoLWVkMjU1MTkgQUFBQUMzTnphQzFsWkRJMU5URTVBQUFBSUExaFZ3QW9ORGE2NCt3b0hZeGtIZmR1NnFtZEMxQnNMWFJlZVF6L0NCcmkgZXhhbXBsZUBkZW1v\n");
        Ok(())
    }

    #[test]
    fn test_packages() -> Result<()> {
        let root = &cap_std_ext::cap_tempfile::TempDir::new(cap_std::ambient_authority())?;

        let blueprint: Blueprint = toml::from_str(indoc! { r#"
            [[packages]]
            name = "httpd"
            version = "2.4"
            [[packages]]
            name = "mariadb-server"
            [[packages]]
            name = "mariadb"  
            [[packages]]
            name = "php"
            version = "5.1"
            "#
        })?;
        let mut r = Rendered::new()?;
        let changed = blueprint.render(root, &mut r).unwrap();
        assert!(changed);
        let dnf = r.exec.pop().unwrap().0;
        let dnf = &dnf.iter().map(|s| s.as_str()).collect::<Vec<_>>();
        assert!(r.exec.is_empty());
        assert_eq!(
            dnf.as_slice(),
            [
                "dnf",
                "install",
                "-y",
                "httpd-2.4",
                "mariadb-server",
                "mariadb",
                "php-5.1"
            ]
        );
        assert_eq!(r.filesystem.entries()?.count(), 0);
        Ok(())
    }
}
