//! Where each person's Hugging Face and Civitai tokens are kept.
//!
//! In Windows Credential Manager, under this Windows account, on this
//! computer. Never in the vault: the vault is a folder a person carries on a
//! drive they might lend, and its database would hold the token in the clear.
//! Never in a log, an error, an event or the journal either. Nothing in the
//! engine formats a token into text, except the one `Authorization` header it
//! is for.

use std::collections::HashMap;
use std::sync::Mutex;

use super::sites::Host;
use crate::error::{ErrorCode, Result, VaultError};

pub trait TokenStore: Send + Sync {
    fn get(&self, host: Host) -> Result<Option<String>>;
    fn set(&self, host: Host, token: &str) -> Result<()>;
    /// Answers whether there was one to remove.
    fn remove(&self, host: Host) -> Result<bool>;
}

/// The longest token accepted. Real ones are well under a hundred characters.
const MAX_LEN: usize = 1024;

/// A token as pasted, trimmed, and held to the characters a token has.
///
/// A token goes into a request header, so a line break in it would let the
/// text after it become a header of its own. Only visible ASCII is accepted.
pub fn clean(token: &str) -> Result<String> {
    let t = token.trim();
    if t.is_empty() {
        return Err(VaultError::invalid("Paste a token first."));
    }
    if t.len() > MAX_LEN || !t.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(VaultError::invalid(
            "That is not a token. A token is one line of letters, digits and signs, with no spaces.",
        ));
    }
    Ok(t.to_string())
}

fn target(prefix: &str, host: Host) -> String {
    let service = match host {
        Host::HuggingFace => "huggingface",
        Host::Civitai => "civitai",
    };
    format!("{prefix}/{service}")
}

/// The store this computer has.
pub fn system_store() -> std::sync::Arc<dyn TokenStore> {
    #[cfg(windows)]
    {
        std::sync::Arc::new(CredentialManager::new("ComfyVault"))
    }
    #[cfg(not(windows))]
    {
        std::sync::Arc::new(NoStore)
    }
}

/// Windows Credential Manager.
#[cfg(windows)]
pub struct CredentialManager {
    prefix: String,
}

#[cfg(windows)]
impl CredentialManager {
    /// `prefix` names the entries: `ComfyVault/huggingface` for the product.
    /// Tests use a prefix of their own, so they never touch the person's.
    pub fn new(prefix: &str) -> Self {
        Self { prefix: prefix.to_string() }
    }
}

#[cfg(windows)]
impl TokenStore for CredentialManager {
    fn get(&self, host: Host) -> Result<Option<String>> {
        win::read(&target(&self.prefix, host))
    }

    fn set(&self, host: Host, token: &str) -> Result<()> {
        win::write(&target(&self.prefix, host), &clean(token)?)
    }

    fn remove(&self, host: Host) -> Result<bool> {
        win::delete(&target(&self.prefix, host))
    }
}

#[cfg(windows)]
mod win {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    use crate::error::{ErrorCode, Result, VaultError};

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    fn failed(doing: &str, code: u32) -> VaultError {
        VaultError::new(
            ErrorCode::PermissionDenied,
            format!("Windows Credential Manager refused {doing}."),
        )
        .with_detail(format!("Windows error {code}"))
    }

    pub fn write(target: &str, token: &str) -> Result<()> {
        let mut name = wide(target);
        let mut user = wide("ComfyVault");
        let mut blob = token.as_bytes().to_vec();
        let cred = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: name.as_mut_ptr(),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            // This Windows account on this computer only. It does not roam.
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: user.as_mut_ptr(),
            ..Default::default()
        };
        // SAFETY: every pointer in `cred` points into a buffer that lives to
        // the end of this function, and the call only reads them.
        let ok = unsafe { CredWriteW(&cred, 0) };
        // The copy in this process is overwritten before it is freed.
        blob.iter_mut().for_each(|b| *b = 0);
        if ok == 0 {
            return Err(failed("to save the token", unsafe { GetLastError() }));
        }
        Ok(())
    }

    pub fn read(target: &str) -> Result<Option<String>> {
        let name = wide(target);
        let mut out: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: `name` is a terminated wide string, and `out` receives a
        // buffer that Windows allocates and `CredFree` releases below.
        let ok = unsafe { CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut out) };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            if code == ERROR_NOT_FOUND {
                return Ok(None);
            }
            return Err(failed("to read the token", code));
        }
        // SAFETY: on success `out` points at one CREDENTIALW whose blob is
        // `CredentialBlobSize` bytes long.
        let token = unsafe {
            let c = &*out;
            let bytes = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec();
            CredFree(out as *const _);
            bytes
        };
        String::from_utf8(token)
            .map(Some)
            .map_err(|_| VaultError::new(ErrorCode::ParseError, "The saved token could not be read. Remove it and save it again."))
    }

    pub fn delete(target: &str) -> Result<bool> {
        let name = wide(target);
        // SAFETY: `name` is a terminated wide string.
        let ok = unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            if code == ERROR_NOT_FOUND {
                return Ok(false);
            }
            return Err(failed("to remove the token", code));
        }
        Ok(true)
    }
}

/// The store on a computer that is not Windows. ComfyVault runs on Windows
/// only; this keeps a development build honest rather than keeping a token
/// somewhere weaker.
pub struct NoStore;

impl TokenStore for NoStore {
    fn get(&self, _host: Host) -> Result<Option<String>> {
        Ok(None)
    }

    fn set(&self, _host: Host, _token: &str) -> Result<()> {
        Err(VaultError::new(
            ErrorCode::PermissionDenied,
            "Tokens are kept in Windows Credential Manager, and this computer does not have it.",
        ))
    }

    fn remove(&self, _host: Host) -> Result<bool> {
        Ok(false)
    }
}

/// Tokens in memory, for tests.
#[derive(Default)]
pub struct MemoryTokens(Mutex<HashMap<String, String>>);

impl TokenStore for MemoryTokens {
    fn get(&self, host: Host) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(&target("t", host)).cloned())
    }

    fn set(&self, host: Host, token: &str) -> Result<()> {
        self.0.lock().unwrap().insert(target("t", host), clean(token)?);
        Ok(())
    }

    fn remove(&self, host: Host) -> Result<bool> {
        Ok(self.0.lock().unwrap().remove(&target("t", host)).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_held_to_one_line_of_visible_characters() {
        assert_eq!(clean("  hf_abc123  ").unwrap(), "hf_abc123");
        for bad in ["", "   ", "hf_abc\r\nX-Evil: 1", "has space", "tab\there", "é"] {
            assert_eq!(clean(bad).unwrap_err().code, ErrorCode::InvalidArgument, "{bad:?}");
        }
        assert!(clean(&"a".repeat(MAX_LEN + 1)).is_err());
    }

    #[test]
    fn an_error_never_quotes_the_token() {
        let e = clean("hf_secret value").unwrap_err();
        assert!(!format!("{e:?}").contains("hf_secret"));
    }

    #[cfg(windows)]
    #[test]
    fn credential_manager_keeps_reads_and_forgets_a_token() {
        // A name of its own, so the person's saved tokens are never touched.
        let prefix = format!("ComfyVault-test-{}", uuid::Uuid::new_v4().simple());
        let store = CredentialManager::new(&prefix);
        assert_eq!(store.get(Host::HuggingFace).unwrap(), None);
        store.set(Host::HuggingFace, "hf_test_token_1").unwrap();
        store.set(Host::Civitai, "civitai_key_2").unwrap();
        assert_eq!(store.get(Host::HuggingFace).unwrap().as_deref(), Some("hf_test_token_1"));
        assert_eq!(store.get(Host::Civitai).unwrap().as_deref(), Some("civitai_key_2"));

        store.set(Host::HuggingFace, "hf_test_token_replaced").unwrap();
        assert_eq!(store.get(Host::HuggingFace).unwrap().as_deref(), Some("hf_test_token_replaced"));

        assert!(store.remove(Host::HuggingFace).unwrap());
        assert!(store.remove(Host::Civitai).unwrap());
        assert_eq!(store.get(Host::HuggingFace).unwrap(), None, "the token is still there");
        assert_eq!(store.get(Host::Civitai).unwrap(), None);
        assert!(!store.remove(Host::Civitai).unwrap(), "a second remove finds nothing");
    }
}
