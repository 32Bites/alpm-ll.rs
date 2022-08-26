use crate::{Alpm, Package, Result};

use alpm_sys_ll::*;

impl Alpm {
    pub fn trans_remove_pkg(&self, pkg: Package) -> Result<()> {
        let ret = unsafe { self.lib.alpm_remove_pkg(self.as_ptr(), pkg.pkg.as_ptr()) };
        self.check_ret(ret)
    }
}
