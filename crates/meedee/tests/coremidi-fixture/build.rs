use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=coremidi.cpp");
    if env::var("CARGO_CFG_TARGET_OS").unwrap() != "macos" {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let object = out.join("coremidi.o");
    assert!(Command::new("xcrun")
        .args([
            "clang++",
            "-std=c++20",
            "-fblocks",
            "-mmacosx-version-min=11.0",
            "-c",
            "coremidi.cpp",
            "-o"
        ])
        .arg(&object)
        .status()
        .unwrap()
        .success());
    assert!(Command::new("ar")
        .arg("crs")
        .arg(out.join("libmeedee_coremidi_fixture.a"))
        .arg(&object)
        .status()
        .unwrap()
        .success());
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=meedee_coremidi_fixture");
    println!("cargo:rustc-link-lib=framework=CoreMIDI");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
    println!("cargo:rustc-link-lib=c++");
}
