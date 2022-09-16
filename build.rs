use std::{
    io::{self, Write},
    process::Command,
};

fn main() {
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
        .expect(&format!("pnpm {cmd} failed"));

    println!("status: {}", out.status);
    io::stderr().write_all(&out.stdout).unwrap();
    io::stderr().write_all(&out.stderr).unwrap();
}
