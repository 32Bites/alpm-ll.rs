use crate::utils::*;
use crate::{Callbacks, Error, Result};

use std::ffi::{c_void, CString};
use std::fmt;
use std::os::raw::c_int;
use std::path::Path;
use std::ptr::NonNull;

use alpm_sys_ll::*;
use bitflags::bitflags;
use libloading::library_filename;
use once_cell::race::OnceBox;

extern "C" {
    pub(crate) fn free(ptr: *mut c_void);
}

pub static LIBRARY: OnceBox<libalpm> = OnceBox::new();

pub trait Library {
    unsafe fn set_lib<P: AsRef<Path>>(&self, path: P) -> Option<&libalpm>;
    unsafe fn load(&self) -> Option<&libalpm>;
    unsafe fn force_load(&self) -> &libalpm {
        self.load().expect("Failed loading libalpm")
    }
}

impl Library for OnceBox<libalpm> {
    unsafe fn set_lib<P: AsRef<Path>>(&self, path: P) -> Option<&libalpm> {
        let library = libalpm::new(path.as_ref()).ok()?;
        self.set(Box::new(library)).ok()?;
        self.get()
    }

    unsafe fn load(&self) -> Option<&libalpm> {
        self.get_or_try_init(|| libalpm::new(library_filename("alpm")).map(|l| Box::new(l))).ok()
    }
}

#[allow(dead_code)]
pub struct Alpm {
    handle: NonNull<alpm_handle_t>,
    pub(crate) cbs: Callbacks,
    pub(crate) lib: &'static libalpm
}

impl std::fmt::Debug for Alpm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Alpm").finish()
    }
}

// TODO unsound: remove
unsafe impl Send for Alpm {}

impl Drop for Alpm {
    fn drop(&mut self) {
        unsafe { self.lib.alpm_release(self.as_ptr()) };
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Ord, PartialOrd, Hash)]
pub struct ReleaseError;

impl fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("failed to release alpm")
    }
}

impl std::error::Error for ReleaseError {}

impl Alpm {
    #[doc(alias("alpm_initialize", "initialize"))]
    pub fn new<S: Into<Vec<u8>>>(root: S, db_path: S) -> Result<Alpm> {
        let mut err = alpm_errno_t::ALPM_ERR_OK;
        let root = CString::new(root).unwrap();
        let db_path = CString::new(db_path).unwrap();
        let library = unsafe { LIBRARY.load().ok_or(Error::FailedLoading)? };

        let handle = unsafe { library.alpm_initialize(root.as_ptr(), db_path.as_ptr(), &mut err) };

        match NonNull::new(handle) {
            None => unsafe { Err(Error::new(err)) },
            Some(handle) => Ok(Alpm {
                handle,
                cbs: Callbacks::default(),
                lib: library
            }),
        }
    }

    pub fn new2(root: &str, db_path: &str) -> Result<Alpm> {
        Alpm::new(root, db_path)
    }

    pub fn release(self) -> std::result::Result<(), ReleaseError> {
        if unsafe { self.lib.alpm_release(self.as_ptr()) } == 0 {
            std::mem::forget(self);
            Ok(())
        } else {
            std::mem::forget(self);
            Err(ReleaseError)
        }
    }

    pub(crate) unsafe fn from_ptr(handle: *mut alpm_handle_t) -> Alpm {
        Alpm {
            handle: NonNull::new_unchecked(handle),
            cbs: Callbacks::default(),
            lib: LIBRARY.force_load()
        }
    }

    pub(crate) fn as_ptr(&self) -> *mut alpm_handle_t {
        self.handle.as_ptr()
    }

    pub(crate) fn check_ret(&self, int: c_int) -> Result<()> {
        if int != 0 {
            Err(self.last_error())
        } else {
            Ok(())
        }
    }

    pub(crate) fn check_null<T>(&self, ptr: *const T) -> Result<()> {
        if ptr.is_null() {
            Err(self.last_error())
        } else {
            Ok(())
        }
    }
}

pub fn version() -> &'static str {
    unsafe { from_cstr(LIBRARY.force_load().alpm_version()) }
}

bitflags! {
    pub struct Capabilities: u32 {
        const NLS = alpm_caps::ALPM_CAPABILITY_NLS;
        const DOWNLOADER = alpm_caps::ALPM_CAPABILITY_DOWNLOADER;
        const SIGNATURES = alpm_caps::ALPM_CAPABILITY_SIGNATURES;
    }
}

impl Default for Capabilities {
    fn default() -> Capabilities {
        Capabilities::new()
    }
}

impl Capabilities {
    pub fn new() -> Capabilities {
        Capabilities::from_bits(unsafe { LIBRARY.force_load().alpm_capabilities() as u32 }).unwrap()
    }

    pub fn nls(self) -> bool {
        self.intersects(Capabilities::NLS)
    }

    pub fn downloader(self) -> bool {
        self.intersects(Capabilities::DOWNLOADER)
    }

    pub fn signatures(self) -> bool {
        self.intersects(Capabilities::SIGNATURES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SigLevel;

    #[test]
    fn test_lifetime() {
        let handle = Alpm::new("/", "tests/db").unwrap();
        let db = handle.register_syncdb("core", SigLevel::NONE).unwrap();
        let pkg = db.pkg("linux").unwrap();
        let name = pkg.name();

        drop(pkg);
        drop(db);
        assert_eq!(name, "linux");
    }

    #[test]
    fn test_list_lifetime() {
        let handle = Alpm::new("/", "tests/db").unwrap();
        let db = handle.register_syncdb("core", SigLevel::NONE).unwrap();
        let pkgs = db.pkgs();

        drop(db);
        assert!(pkgs.len() > 10);
    }
}