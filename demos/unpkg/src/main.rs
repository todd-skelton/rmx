//! `server.ts`: PORT (44109) and TMP_DIR (tarball cache, default demos/unpkg/tmp).

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    use rmx::file_storage::FsFileStorage;
    use unpkg_demo::fetch::Fetch;
    use unpkg_demo::npm::Npm;

    rmx::router::routes::routes_command(&*unpkg_demo::ROUTES);
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44109".into());
    let tmp = std::path::PathBuf::from(std::env::var("TMP_DIR").unwrap_or_else(|_| "demos/unpkg/tmp".into()));
    let cache = FsFileStorage::new(tmp.join("tarballs")).expect("tarball cache directory");
    let addr = format!("{host}:{port}");
    println!("unpkg demo is running on http://{addr}");
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(rmx::node_fetch_server::listen(&addr, unpkg_demo::router(Npm::new(Fetch::network(), cache)))).expect("serve");
}

#[cfg(target_arch = "wasm32")]
fn main() {}
