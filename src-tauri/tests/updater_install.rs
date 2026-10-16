#![cfg(target_os = "macos")]

use serde_json::json;
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri_plugin_updater::{Error, UpdaterExt};

struct ReleaseServer {
    address: SocketAddr,
    stopped: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ReleaseServer {
    fn start(version: &str, signature: &str, artifact: &[u8]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let manifest = |version: &str, file: &str| {
            serde_json::to_vec(&json!({
                "version": version,
                "platforms": {
                    "darwin-aarch64": {"url": format!("http://{address}/{file}"), "signature": signature},
                    "darwin-x86_64": {"url": format!("http://{address}/{file}"), "signature": signature}
                }
            }))
            .unwrap()
        };
        let mut tampered = artifact.to_vec();
        *tampered.last_mut().expect("Release artifact is empty") ^= 1;
        let responses = HashMap::from([
            ("/latest.json", manifest(version, "Margin.app.tar.gz")),
            ("/tampered.json", manifest(version, "tampered.tar.gz")),
            ("/mismatch.json", manifest("999.0.0", "Margin.app.tar.gz")),
            ("/Margin.app.tar.gz", artifact.to_vec()),
            ("/tampered.tar.gz", tampered),
        ]);
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = stopped.clone();
        let worker = thread::spawn(move || {
            for connection in listener.incoming() {
                if worker_stopped.load(Ordering::SeqCst) {
                    break;
                }
                let mut stream = connection.unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut request = Vec::new();
                let mut chunk = [0; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let length = stream.read(&mut chunk).unwrap();
                    assert!(length > 0 && request.len() + length <= 16_384);
                    request.extend_from_slice(&chunk[..length]);
                }
                let request = String::from_utf8(request).unwrap();
                let mut words = request.split_whitespace();
                assert_eq!(words.next(), Some("GET"));
                let path = words.next().unwrap();
                let body = responses.get(path).expect("Unexpected loopback request");
                let content_type = if path.ends_with(".json") {
                    "application/json"
                } else {
                    "application/octet-stream"
                };
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                stream.write_all(body).unwrap();
            }
        });
        Self {
            address,
            stopped,
            worker: Some(worker),
        }
    }

    fn url(&self, path: &str) -> reqwest::Url {
        format!("http://{}{path}", self.address).parse().unwrap()
    }
}

impl Drop for ReleaseServer {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !thread::panicking() {
                result.unwrap();
            }
        }
    }
}

fn command_output(program: &str, arguments: &[&str], path: &Path) -> String {
    let output = Command::new(program)
        .args(arguments)
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{program} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the exact signed macOS release archive in MARGIN_UPDATE_ARTIFACT_DIR"]
async fn signed_release_installs_into_a_disposable_app() {
    let artifact_directory = PathBuf::from(
        std::env::var_os("MARGIN_UPDATE_ARTIFACT_DIR")
            .expect("Set MARGIN_UPDATE_ARTIFACT_DIR to downloaded release assets"),
    );
    let artifact = fs::read(artifact_directory.join("Margin.app.tar.gz")).unwrap();
    let signature = fs::read_to_string(artifact_directory.join("Margin.app.tar.gz.sig")).unwrap();
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let version = config["version"].as_str().unwrap();
    let public_key = config["plugins"]["updater"]["pubkey"].as_str().unwrap();
    assert_eq!(config["plugins"]["updater"]["requireSignedVersion"], true);
    let server = ReleaseServer::start(version, signature.trim(), &artifact);

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".cache/upgrade-smoke");
    fs::create_dir_all(&root).unwrap();
    let disposable = tempfile::Builder::new()
        .prefix("signed-install-")
        .tempdir_in(root)
        .unwrap();
    let app_path = disposable.path().join("Margin.app");
    let executable = app_path.join("Contents/MacOS/margin");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, "disposable old executable").unwrap();
    let sentinel = app_path.join("old-version-sentinel.txt");
    fs::write(&sentinel, "old app must survive rejected updates").unwrap();

    let mut context = mock_context(noop_assets());
    context.package_info_mut().version = "0.0.0".parse().unwrap();
    context.config_mut().plugins.0.insert(
        "updater".into(),
        json!({
            "pubkey": public_key,
            "requireSignedVersion": true,
            "dangerousInsecureTransportProtocol": true,
            "endpoints": [server.url("/latest.json")]
        }),
    );
    let app = mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .unwrap();
    let updater = |path: &str| {
        app.updater_builder()
            .executable_path(executable.clone())
            .endpoints(vec![server.url(path)])
            .unwrap()
            .no_proxy()
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap()
    };

    let update = updater("/latest.json").check().await.unwrap().unwrap();
    assert_eq!(update.version, version);
    let downloaded = update.download(|_, _| {}, || {}).await.unwrap();
    assert_eq!(downloaded, artifact);

    let tampered_update = updater("/tampered.json").check().await.unwrap().unwrap();
    assert!(matches!(
        tampered_update.download(|_, _| {}, || {}).await,
        Err(Error::Minisign(_))
    ));
    let mut tampered = artifact.clone();
    *tampered.last_mut().unwrap() ^= 1;
    assert!(matches!(update.install(tampered), Err(Error::Minisign(_))));
    assert_eq!(
        fs::read_to_string(&sentinel).unwrap(),
        "old app must survive rejected updates"
    );

    let mismatch = updater("/mismatch.json").check().await.unwrap().unwrap();
    assert!(matches!(
        mismatch.download(|_, _| {}, || {}).await,
        Err(Error::SignedVersionMismatch { .. })
    ));
    assert!(matches!(
        mismatch.install(&artifact),
        Err(Error::SignedVersionMismatch { .. })
    ));
    assert!(sentinel.exists());

    tokio::task::spawn_blocking(move || update.install(downloaded))
        .await
        .unwrap()
        .unwrap();
    assert!(!sentinel.exists());
    assert!(executable.is_file());
    let installed_version = command_output(
        "/usr/bin/plutil",
        &["-extract", "CFBundleShortVersionString", "raw", "-o", "-"],
        &app_path.join("Contents/Info.plist"),
    );
    assert_eq!(installed_version, version);
    command_output(
        "/usr/bin/codesign",
        &["--verify", "--deep", "--strict"],
        &app_path,
    );
    let architectures = command_output("/usr/bin/lipo", &["-archs"], &executable);
    let current_architecture = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    assert!(architectures
        .split_whitespace()
        .any(|arch| arch == current_architecture));
    if std::env::var("MARGIN_REQUIRE_UNIVERSAL").as_deref() == Ok("1") {
        assert!(architectures.split_whitespace().any(|arch| arch == "arm64"));
        assert!(architectures
            .split_whitespace()
            .any(|arch| arch == "x86_64"));
    }
    let retained = disposable.keep();
    println!(
        "Verified signature failures, signed-version failures, and {version} install ({architectures}). Disposable app retained: {}",
        retained.join("Margin.app").display()
    );
}
