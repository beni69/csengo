use const_gen::*;
use std::{
    env,
    fs::write,
    io::{self, Write},
    path::Path,
    process::Command,
};

fn main() {
    // embed git info
    let mut git_ref = git_exec(&["rev-parse", "HEAD"])[0..7].to_string();
    let dirty = !git_exec(&["status", "--short"]).is_empty();
    if dirty {
        git_ref += "*";
    }

    let code = const_declaration!(GIT_REF = git_ref);

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("const_gen.rs");
    write(dest_path, code).unwrap();

    //build fontend
    //
    // for some weird reason ci builds fail on this step on windows
    #[cfg(windows)]
    if std::env::var("CI").is_ok() {
        return;
    }

    println!("cargo:rerun-if-changed=frontend");

    pnpm_exec("install");
    pnpm_exec("build");

    println!("cargo:rerun-if-changed=.git/refs");
}

fn pnpm_exec(cmd: &str) {
    println!("running: pnpm {cmd}");
    let out = Command::new("pnpm")
        .arg(cmd)
        .current_dir("frontend")
        .output()
        .unwrap();

    println!("status: {}", out.status);
    io::stderr().write_all(&out.stdout).unwrap();
    io::stderr().write_all(&out.stderr).unwrap();
}

fn git_exec(cmd: &[&str]) -> String {
    println!("running: git {}", cmd.join(" "));
    let res = Command::new("git").args(cmd).output().unwrap();
    String::from_utf8(res.stdout).unwrap()
}
