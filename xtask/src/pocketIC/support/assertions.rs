use anyhow::{bail, Result};

pub fn require_ignored_flag() -> Result<()> {
    Ok(())
}

pub fn assert_eventually(description: &str, condition: bool) -> Result<()> {
    if condition {
        Ok(())
    } else {
        bail!("{description}")
    }
}
