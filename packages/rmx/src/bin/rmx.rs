//! The `rmx` command: `rmx new my-app`, `rmx routes`, `rmx help`. See [`rmx::cli`].

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    std::process::exit(rmx::cli::run(&args, &cwd));
}

#[cfg(target_arch = "wasm32")]
fn main() {}
