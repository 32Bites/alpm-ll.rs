use alpm_ll::{Alpm, SigLevel, TransFlag};

fn main() {
    // initialise the handle
    let mut handle = Alpm::new("/", "tests/db").unwrap();

    // configure any settings
    // handle.set_ignorepkgs(["a", "b", "c"].iter()).unwrap();
    // handle.add_cachedir("/var/lib/pacman").unwrap();
    handle.set_check_space(true);
    handle.add_architecture("x86_64").unwrap();

    // register any databases you wish to use
    handle
        .register_syncdb("core", SigLevel::USE_DEFAULT)
        .unwrap();
    handle
        .register_syncdb("extra", SigLevel::USE_DEFAULT)
        .unwrap();
    handle
        .register_syncdb("community", SigLevel::USE_DEFAULT)
        .unwrap();
        handle.syncdbs_mut().iter().for_each(|d| {
            d.add_server(format!("https://america.mirror.pkgbuild.com/{}/os/x86_64", d.name())).unwrap();
        });
        handle.syncdbs_mut().update(true).unwrap();

    handle.syncdbs().iter().find(|d| d.name() == "core").unwrap().pkgs().iter().for_each(|p| println!("{}", p.name()))
}
