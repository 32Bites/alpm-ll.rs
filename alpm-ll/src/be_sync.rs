use crate::{Result, LIBRARY, Library};
use alpm_sys_ll::*;

use crate::{AlpmList, DbMut};

impl<'a> AlpmList<'a, DbMut<'a>> {
    pub fn update(&self, force: bool) -> Result<bool> {
        let force = if force { 1 } else { 0 };
        let ret = unsafe { LIBRARY.force_load().alpm_db_update(self.handle.as_ptr(), self.list, force) };
        if ret == -1 {
            Err(self.handle.last_error())
        } else {
            Ok(ret == 1)
        }
    }
}
