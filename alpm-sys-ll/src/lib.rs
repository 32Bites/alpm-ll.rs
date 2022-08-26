#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

#[cfg(not(any(feature = "generate")))]
mod ffi;

#[cfg(feature = "generate")]
mod ffi_generated;

#[cfg(not(any(feature = "generate")))]
pub use crate::ffi::*;


#[cfg(feature = "generate")]
pub use crate::ffi_generated::*;
