include!(concat!(env!("OUT_DIR"), "/ffi_generated.rs"));

#[cfg(test)]
mod test {
    #[cfg(feature = "runtime")]
    #[test]
    fn test() {
        unsafe {
            // super::
            let lib = super::libalpm::new("libalpm").unwrap();
        }
    }
}