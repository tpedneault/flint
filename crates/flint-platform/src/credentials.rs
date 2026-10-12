//! Secrets in the OS credential store: Windows Credential Manager, the
//! Secret Service (KWallet, GNOME Keyring) on Linux. Never in
//! `settings.toml`, which people copy, sync and paste into bug reports.
//!
//! Entries are named as FENIX-INTEGRATION.md's F12 agrees for the whole
//! suite: service `fenix-suite`, account `<kind>:<where>`
//! (`jira:https://jira.example.com`, `gitlab:https://gitlab.example.com`).
//! Once Fenix keeps its tokens there too, both find the same token and
//! the user enters it once. Until then Flint asks for its own, under the
//! same names; it never reads Fenix's `settings.toml` for one.
//!
//! Flint only stores and fetches. Which provider gets which secret is
//! `[[providers]] credential`'s business, and the provider receives it in
//! its environment when Flint starts it (docs/PROVIDERS.md), so no
//! provider talks to the store itself.

/// The service every suite tool files its secrets under (F12).
pub const SERVICE: &str = "fenix-suite";

fn entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, account)
        .map_err(|e| format!("the credential store refused `{account}`: {e}"))
}

/// The secret stored as `account`, `None` when there is none.
pub fn get(account: &str) -> Result<Option<String>, String> {
    match entry(account)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!(
            "couldn't read `{account}` from the credential store: {e}"
        )),
    }
}

/// Stores `secret` as `account`, replacing what was there.
pub fn set(account: &str, secret: &str) -> Result<(), String> {
    entry(account)?
        .set_password(secret)
        .map_err(|e| format!("couldn't store `{account}` in the credential store: {e}"))
}

/// Removes `account`'s secret. Already gone is fine.
pub fn delete(account: &str) -> Result<(), String> {
    match entry(account)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!(
            "couldn't remove `{account}` from the credential store: {e}"
        )),
    }
}

// Not unit-tested: storing for real would write to the credential store
// of whoever runs the tests. The names are checked in flint-config
// (`check_credential_name`); the round trip is checked on a desktop.
