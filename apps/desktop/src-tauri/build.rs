fn main() {
    // ggml-metal's Objective-C code (via whisper-rs `metal` feature) uses
    // `@available` checks, which reference `___isPlatformVersionAtLeast` from
    // clang's compiler-rt. rustc's linker invocation doesn't pull that
    // builtins archive in automatically, so locate it via clang and link it
    // explicitly. No-op when clang is unavailable or the file is missing.
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("clang")
            .arg("--print-libgcc-file-name")
            .output()
        {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() && std::path::Path::new(&path).exists() {
                println!("cargo:rustc-link-arg={path}");
            }
        }
    }

    tauri_build::build()
}
