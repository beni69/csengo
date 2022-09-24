use std::{
    io::{self, Write},
    process::Command,
};

fn main() {
    // for some weird reason ci builds fail on this step on windows
    #[cfg(windows)]
    if std::env::var("CI").is_ok() {
        return;
    }

    println!("cargo:rerun-if-changed=frontend/src");

    pnpm_exec("install");
    pnpm_exec("build");
}

fn pnpm_exec(cmd: &str) {
    println!("running: pnpm {cmd}");
    let out = Command::new("pnpm")
        .arg(cmd)
        .current_dir("frontend")
        .output()
        .unwrap_or_else(|_| panic!("pnpm {cmd} failed"));

    println!("status: {}", out.status);
    io::stderr().write_all(&out.stdout).unwrap();
    io::stderr().write_all(&out.stderr).unwrap();
}
