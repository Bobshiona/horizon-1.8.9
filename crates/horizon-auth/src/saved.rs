//! The saved Microsoft sign-in: one refresh token per client ID, kept as a
//! generic password in the user's macOS Keychain. Nothing is written to disk
//! by this crate, and errors carry the Keychain status code only.
//!
//! Other platforms have no store yet: nothing loads and saving is refused.

use std::io;

/// The Keychain item's service name, as Keychain Access lists it.
pub const SERVICE: &str = "Horizon 1.8.9 Microsoft sign-in";

/// The saved refresh token for `client_id`, if there is one.
pub fn load(client_id: &str) -> io::Result<Option<String>> {
    store::load(SERVICE, client_id)
}

/// Save (or replace) the refresh token for `client_id`.
pub fn store(client_id: &str, refresh_token: &str) -> io::Result<()> {
    if refresh_token.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Refresh token is empty",
        ));
    }
    store::save(SERVICE, client_id, refresh_token)
}

/// Remove the saved sign-in for `client_id`; false when there was none.
pub fn forget(client_id: &str) -> io::Result<bool> {
    store::delete(SERVICE, client_id)
}

#[cfg(target_os = "macos")]
mod store {
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };
    use std::io;

    /// `errSecItemNotFound`.
    const NOT_FOUND: i32 = -25300;

    fn error(action: &str, error: security_framework::base::Error) -> io::Error {
        io::Error::other(format!(
            "Keychain could not {action} the saved sign-in (OSStatus {})",
            error.code()
        ))
    }

    pub fn load(service: &str, account: &str) -> io::Result<Option<String>> {
        match get_generic_password(service, account) {
            Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "Saved sign-in is not text")
            }),
            Err(failure) if failure.code() == NOT_FOUND => Ok(None),
            Err(failure) => Err(error("read", failure)),
        }
    }

    pub fn save(service: &str, account: &str, secret: &str) -> io::Result<()> {
        set_generic_password(service, account, secret.as_bytes()).map_err(|e| error("save", e))
    }

    pub fn delete(service: &str, account: &str) -> io::Result<bool> {
        match delete_generic_password(service, account) {
            Ok(()) => Ok(true),
            Err(failure) if failure.code() == NOT_FOUND => Ok(false),
            Err(failure) => Err(error("remove", failure)),
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod store {
    use std::io;

    pub fn load(_: &str, _: &str) -> io::Result<Option<String>> {
        Ok(None)
    }

    pub fn save(_: &str, _: &str, _: &str) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Saving the sign-in needs the macOS Keychain",
        ))
    }

    pub fn delete(_: &str, _: &str) -> io::Result<bool> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tokens_are_never_saved() {
        assert_eq!(
            store("client", "").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    /// Writes, reads and removes a throwaway item in the login Keychain, so it
    /// runs only with `HORIZON_KEYCHAIN_TEST=1` on an unlocked Mac.
    #[test]
    fn keychain_round_trip_replaces_and_forgets() {
        if std::env::var_os("HORIZON_KEYCHAIN_TEST").is_none() {
            return;
        }
        let service = "Horizon 1.8.9 Keychain test";
        let account = "round-trip";
        let _ = store::delete(service, account);
        assert_eq!(store::load(service, account).unwrap(), None);
        store::save(service, account, "first").unwrap();
        store::save(service, account, "second").unwrap();
        assert_eq!(
            store::load(service, account).unwrap().as_deref(),
            Some("second")
        );
        assert!(store::delete(service, account).unwrap());
        assert!(!store::delete(service, account).unwrap());
        assert_eq!(store::load(service, account).unwrap(), None);
    }
}
