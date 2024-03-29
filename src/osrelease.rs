use anyhow::Result;
use fn_error_context::context;

#[context("Verifying os-release")]
pub(crate) fn verify_osrelease() -> Result<()> {
    let osrelease = std::fs::read_to_string("/usr/lib/os-release")?;
    let mut is_fedora_idlike = false;
    let mut osid = None;
    for line in osrelease.lines() {
        let (k, v) = if let Some(v) = line.split_once('=') {
            v
        } else {
            continue;
        };
        let v = v.trim();
        match k.trim() {
            "ID" => osid = Some(v),
            "ID_LIKE" => {
                if v.contains("fedora") {
                    is_fedora_idlike = true;
                }
            }
            _ => continue,
        }
    }
    if !(is_fedora_idlike || matches!(osid, Some("fedora"))) {
        anyhow::bail!("ID/ID_LIKE does not contain fedora, unsupported OS {osid:?}");
    }
    Ok(())
}
