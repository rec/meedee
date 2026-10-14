use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=native");
    println!("cargo:rerun-if-changed=../../third_party/libremidi");
    for name in [
        "CMAKE_TOOLCHAIN_FILE",
        "CMAKE_GENERATOR",
        "CC",
        "CXX",
        "MACOSX_DEPLOYMENT_TARGET",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();
    let vendor = manifest.join("../../third_party/libremidi").canonicalize()
        .expect("build from the complete meedee checkout or source distribution: vendored libremidi is missing");
    let mut configure = Command::new("cmake");
    configure
        .args(["-S"])
        .arg(manifest.join("native"))
        .arg("-B")
        .arg(output.join("native"))
        .arg(format!("-DMEEDEE_VENDOR={}", vendor.display()))
        .arg(format!("-DCMAKE_INSTALL_PREFIX={}", output.display()))
        .args(["-DCMAKE_BUILD_TYPE=Release", "-DCMAKE_INSTALL_LIBDIR=lib"]);
    if let Some(toolchain) = env::var_os("CMAKE_TOOLCHAIN_FILE") {
        configure.arg(format!(
            "-DCMAKE_TOOLCHAIN_FILE={}",
            PathBuf::from(toolchain).display()
        ));
    } else {
        assert_eq!(
            target,
            env::var("HOST").unwrap(),
            "cross builds require CMAKE_TOOLCHAIN_FILE for the target C++ compiler and sysroot"
        );
    }
    if target.contains("apple") {
        configure.arg(format!(
            "-DCMAKE_OSX_DEPLOYMENT_TARGET={}",
            env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| "11.0".into())
        ));
    }
    run(&mut configure);
    run(Command::new("cmake")
        .arg("--build")
        .arg(output.join("native"))
        .args(["--config", "Release", "--target", "install", "--parallel"])
        .arg(env::var("NUM_JOBS").unwrap_or_else(|_| "2".into())));
    println!("cargo:rustc-link-search=native={}/lib", output.display());
    println!("cargo:rustc-link-lib=static=meedee_native");
    println!("cargo:rustc-link-lib=static=libremidi");
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
        for framework in ["CoreMIDI", "CoreAudio", "CoreFoundation"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    } else if target.contains("linux") {
        println!("cargo:rustc-link-lib=stdc++");
        println!("cargo:rustc-link-lib=dl");
    } else if target.contains("windows") {
        println!("cargo:rustc-link-lib=winmm");
        println!("cargo:rustc-link-lib=ole32");
    }
}

fn run(command: &mut Command) {
    let result = command
        .status()
        .expect("native builds require CMake and a C++20 toolchain");
    assert!(result.success(), "native build failed: {command:?}");
}
