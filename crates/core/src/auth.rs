use crate::error::{Error, Result};
use simplenote_cli::{api::Http, auth};
use std::path::Path;

/// Page where the user obtains the `email:CODE` credential.
pub fn login_url() -> &'static str {
    auth::LOGIN_URL
}

/// Exchange the browser credential for a sync token and persist it (0600).
pub fn login(dir: &Path, credential: &str) -> Result<()> {
    let http = Http::new().map_err(|e| Error::msg(e.message))?;
    let token = auth::redeem(&http, credential).map_err(|e| Error::msg(e.message))?;
    auth::save(dir, token).map_err(|e| Error::msg(e.message))
}

pub fn token(dir: &Path) -> Option<String> {
    auth::read(dir).ok()
}

pub fn logout(dir: &Path) -> Result<()> {
    auth::logout(dir).map(|_| ()).map_err(|e| Error::msg(e.message))
}
