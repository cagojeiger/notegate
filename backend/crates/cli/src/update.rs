use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use fd_lock::RwLock;
use reqwest::Url;
use reqwest::redirect::{Attempt, Policy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{CliError, UpdateArgs};

const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/cagojeiger/notegate/releases/latest/download/notegate-cli-manifest.json";
const DEFAULT_REPOSITORY: &str = "cagojeiger/notegate";
const MANIFEST_SCHEMA_VERSION: u32 = 1;
const RECEIPT_SCHEMA_VERSION: u32 = 1;
const RECEIPT_MANAGED_BY: &str = "notegate-cli-installer";
const MANIFEST_MAX_BYTES: usize = 256 * 1024;
const ARTIFACT_MAX_BYTES: usize = 100 * 1024 * 1024;

pub(crate) async fn run(args: UpdateArgs, timeout: Duration) -> Result<Value, CliError> {
    let current_exe = std::env::current_exe().map_err(|_error| {
        CliError::configuration(
            "current_executable_unavailable",
            "could not determine the running notegate-cli executable path",
        )
    })?;
    let settings = UpdateSettings {
        receipt_path: default_receipt_path(&current_exe),
        manifest_url: DEFAULT_MANIFEST_URL.to_owned(),
        asset_base_url: None,
        check_only: args.check,
        timeout,
        current_exe,
    };
    run_with_settings(settings).await
}

#[derive(Clone, Debug)]
struct UpdateSettings {
    current_exe: PathBuf,
    receipt_path: PathBuf,
    manifest_url: String,
    asset_base_url: Option<String>,
    check_only: bool,
    timeout: Duration,
}

async fn run_with_settings(settings: UpdateSettings) -> Result<Value, CliError> {
    let target = current_target()?;
    let receipt = read_receipt(&settings.receipt_path)?;
    validate_receipt(&receipt, &settings.current_exe, &target)?;

    let client = reqwest::Client::builder()
        .timeout(settings.timeout)
        .redirect(safe_redirect_policy())
        .build()
        .map_err(|_error| {
            CliError::unavailable(
                "update_client_failed",
                "could not create the update HTTP client",
            )
        })?;
    let manifest_url = safe_download_url(&settings.manifest_url, "update manifest URL")?;
    let manifest_bytes = download_bounded(&client, manifest_url, MANIFEST_MAX_BYTES).await?;
    let manifest: UpdateManifest = serde_json::from_slice(&manifest_bytes).map_err(|_error| {
        CliError::protocol(
            "invalid_update_manifest",
            "the notegate-cli update manifest was not valid JSON",
        )
    })?;
    manifest.validate()?;
    let artifact = manifest.assets.get(&target).ok_or_else(|| {
        CliError::protocol(
            "missing_update_artifact",
            format!("the notegate-cli update manifest does not include target {target}"),
        )
    })?;

    let current_version = env!("CARGO_PKG_VERSION");
    let update_available = version_is_newer(&manifest.version, current_version)?;
    if !update_available {
        if !settings.check_only && receipt.installed_version != current_version {
            let mut repaired_receipt = receipt.clone();
            repaired_receipt.installed_version = current_version.to_owned();
            write_receipt(&settings.receipt_path, &repaired_receipt)?;
        }
        return Ok(json!({
            "status": "up_to_date",
            "update_available": false,
            "updated": false,
            "current_version": current_version,
            "latest_version": manifest.version,
            "target": target,
        }));
    }
    if settings.check_only {
        return Ok(json!({
            "status": "update_available",
            "update_available": true,
            "updated": false,
            "current_version": current_version,
            "latest_version": manifest.version,
            "target": target,
            "artifact": artifact.name,
        }));
    }

    let parent = settings.current_exe.parent().ok_or_else(|| {
        CliError::configuration(
            "invalid_install_path",
            "the running notegate-cli path has no parent directory",
        )
    })?;
    let lock_path = parent.join(".notegate-cli-update.lock");
    let mut lock = lock_file(&lock_path)?;
    let _guard = match lock.try_write() {
        Ok(guard) => guard,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            return Err(CliError::unavailable(
                "update_in_progress",
                "another notegate-cli update is already in progress",
            ));
        }
        Err(_error) => {
            return Err(CliError::configuration(
                "update_lock_unavailable",
                "could not lock the notegate-cli install directory",
            ));
        }
    };

    let artifact_url = artifact_url(&manifest, artifact, settings.asset_base_url.as_deref())?;
    let bytes = download_bounded(&client, artifact_url, ARTIFACT_MAX_BYTES).await?;
    if bytes.len() != artifact.size {
        return Err(CliError::protocol(
            "update_size_mismatch",
            "the downloaded notegate-cli artifact size did not match the manifest",
        ));
    }
    let actual_sha256 = sha256_hex(&bytes);
    if actual_sha256 != artifact.sha256 {
        return Err(CliError::protocol(
            "update_checksum_mismatch",
            "the downloaded notegate-cli artifact checksum did not match the manifest",
        ));
    }

    reject_symlink(&settings.current_exe)?;
    let temp_path = write_candidate(parent, &bytes)?;
    smoke_check_candidate(&temp_path, &manifest.version)?;
    replace_binary(&temp_path, &settings.current_exe)?;
    sync_directory(parent)?;
    write_receipt(
        &settings.receipt_path,
        &InstallReceipt {
            schema_version: RECEIPT_SCHEMA_VERSION,
            managed_by: RECEIPT_MANAGED_BY.to_owned(),
            repository: manifest.repository.clone(),
            install_path: settings.current_exe.to_string_lossy().to_string(),
            target: target.clone(),
            installed_version: manifest.version.clone(),
        },
    )
    .map_err(|error| update_applied_receipt_failed(error, &manifest.version))?;

    Ok(json!({
        "status": "updated",
        "update_available": true,
        "updated": true,
        "previous_version": current_version,
        "version": manifest.version,
        "target": target,
        "path": settings.current_exe,
    }))
}

#[derive(Debug, Deserialize, Serialize)]
struct UpdateManifest {
    schema_version: u32,
    version: String,
    repository: String,
    assets: BTreeMap<String, ManifestAsset>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ManifestAsset {
    name: String,
    sha256: String,
    size: usize,
}

impl UpdateManifest {
    fn validate(&self) -> Result<(), CliError> {
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(CliError::protocol(
                "unsupported_update_manifest",
                "the notegate-cli update manifest schema is not supported",
            ));
        }
        validate_version(&self.version)?;
        if self.repository != DEFAULT_REPOSITORY {
            return Err(CliError::protocol(
                "unsupported_update_repository",
                "the notegate-cli update manifest repository is not supported",
            ));
        }
        if self.assets.is_empty() {
            return Err(CliError::protocol(
                "empty_update_manifest",
                "the notegate-cli update manifest did not include any assets",
            ));
        }
        for asset in self.assets.values() {
            validate_asset(asset)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct InstallReceipt {
    schema_version: u32,
    managed_by: String,
    repository: String,
    install_path: String,
    target: String,
    installed_version: String,
}

fn validate_receipt(
    receipt: &InstallReceipt,
    current_exe: &Path,
    target: &str,
) -> Result<(), CliError> {
    if receipt.schema_version != RECEIPT_SCHEMA_VERSION
        || receipt.managed_by != RECEIPT_MANAGED_BY
        || receipt.repository != DEFAULT_REPOSITORY
        || validate_version(&receipt.installed_version).is_err()
    {
        return Err(unmanaged_install());
    }
    if receipt.target != target {
        return Err(CliError::configuration(
            "install_target_mismatch",
            "the notegate-cli install receipt target does not match this executable",
        ));
    }
    reject_symlink(Path::new(&receipt.install_path))?;
    reject_symlink(current_exe)?;
    let receipt_path = canonicalize_existing(Path::new(&receipt.install_path))?;
    let current_path = canonicalize_existing(current_exe)?;
    if receipt_path != current_path {
        return Err(CliError::configuration(
            "install_path_mismatch",
            "the notegate-cli install receipt does not match the running executable",
        ));
    }
    Ok(())
}

fn read_receipt(path: &Path) -> Result<InstallReceipt, CliError> {
    let bytes = fs::read(path).map_err(|_error| unmanaged_install())?;
    serde_json::from_slice(&bytes).map_err(|_error| unmanaged_install())
}

fn write_receipt(path: &Path, receipt: &InstallReceipt) -> Result<(), CliError> {
    let parent = path.parent().ok_or_else(|| {
        CliError::configuration(
            "invalid_receipt_path",
            "the notegate-cli install receipt path has no parent directory",
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        CliError::configuration(
            "receipt_write_failed",
            format!("could not create {}: {error}", parent.display()),
        )
    })?;
    let bytes = serde_json::to_vec_pretty(receipt).map_err(|_error| {
        CliError::protocol(
            "receipt_serialization_failed",
            "could not serialize the notegate-cli install receipt",
        )
    })?;
    let (temp_path, mut file) = create_receipt_temp(parent)?;
    {
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| {
                let _ = fs::remove_file(&temp_path);
                CliError::configuration(
                    "receipt_write_failed",
                    format!("could not write {}: {error}", temp_path.display()),
                )
            })?;
    }
    fs::rename(&temp_path, path).map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        CliError::configuration(
            "receipt_write_failed",
            format!("could not write {}: {error}", path.display()),
        )
    })?;
    sync_directory(parent)
}

fn create_receipt_temp(parent: &Path) -> Result<(PathBuf, File), CliError> {
    for attempt in 0..32 {
        let path = parent.join(format!(
            ".notegate-cli-install-{}-{attempt}.tmp",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(CliError::configuration(
                    "receipt_write_failed",
                    format!("could not create {}: {error}", path.display()),
                ));
            }
        }
    }
    Err(CliError::configuration(
        "receipt_write_failed",
        "could not create a unique temporary notegate-cli install receipt",
    ))
}

fn unmanaged_install() -> CliError {
    CliError::configuration(
        "unmanaged_install",
        "notegate-cli was not installed by the official installer; reinstall with the official installer before using update",
    )
}

async fn download_bounded(
    client: &reqwest::Client,
    url: Url,
    max_bytes: usize,
) -> Result<Vec<u8>, CliError> {
    let mut response = client.get(url).send().await.map_err(|error| {
        CliError::unavailable(
            "update_download_failed",
            format!("could not download notegate-cli update metadata or artifact: {error}"),
        )
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(CliError::unavailable(
            "update_download_rejected",
            format!("notegate-cli update download returned HTTP {status}"),
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(CliError::protocol(
            "update_download_too_large",
            "the notegate-cli update download exceeded the safety limit",
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        CliError::unavailable(
            "update_download_failed",
            format!("could not read the notegate-cli update download: {error}"),
        )
    })? {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(CliError::protocol(
                "update_download_too_large",
                "the notegate-cli update download exceeded the safety limit",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn artifact_url(
    manifest: &UpdateManifest,
    artifact: &ManifestAsset,
    asset_base_url: Option<&str>,
) -> Result<Url, CliError> {
    let base = asset_base_url.map(ToOwned::to_owned).unwrap_or_else(|| {
        format!(
            "https://github.com/{}/releases/download/v{}/",
            manifest.repository, manifest.version
        )
    });
    let base = safe_download_url(&base, "update artifact base URL")?;
    base.join(&artifact.name).map_err(|_error| {
        CliError::protocol(
            "invalid_update_artifact_url",
            "could not construct the notegate-cli update artifact URL",
        )
    })
}

fn safe_download_url(input: &str, name: &str) -> Result<Url, CliError> {
    let url = Url::parse(input).map_err(|_error| {
        CliError::configuration("invalid_update_url", format!("{name} is not a valid URL"))
    })?;
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(CliError::configuration(
            "invalid_update_url",
            format!("{name} must not include user info or a fragment"),
        ));
    }
    match url.scheme() {
        "https" => Ok(url),
        "http" if is_loopback_host(&url) => Ok(url),
        _ => Err(CliError::configuration(
            "invalid_update_url",
            format!("{name} must use HTTPS, except loopback HTTP for local tests"),
        )),
    }
}

fn is_loopback_host(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

fn write_candidate(parent: &Path, bytes: &[u8]) -> Result<PathBuf, CliError> {
    for attempt in 0..32 {
        let path = parent.join(format!(
            ".notegate-cli-update-{}-{attempt}.tmp",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = match options.open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(CliError::configuration(
                    "candidate_write_failed",
                    format!("could not create {}: {error}", path.display()),
                ));
            }
        };
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| {
                let _ = fs::remove_file(&path);
                CliError::configuration(
                    "candidate_write_failed",
                    format!("could not write {}: {error}", path.display()),
                )
            })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).map_err(|error| {
                let _ = fs::remove_file(&path);
                CliError::configuration(
                    "candidate_write_failed",
                    format!("could not make {} executable: {error}", path.display()),
                )
            })?;
        }
        return Ok(path);
    }
    Err(CliError::configuration(
        "candidate_write_failed",
        "could not create a unique temporary notegate-cli update file",
    ))
}

fn reject_symlink(path: &Path) -> Result<(), CliError> {
    let metadata = fs::symlink_metadata(path).map_err(|_error| unmanaged_install())?;
    if metadata.file_type().is_symlink() {
        return Err(CliError::configuration(
            "install_path_symlink",
            "the notegate-cli install receipt must point to the real binary, not a symlink",
        ));
    }
    Ok(())
}

fn update_applied_receipt_failed(error: CliError, version: &str) -> CliError {
    let message = error
        .body()
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("could not update the notegate-cli install receipt");
    CliError::unavailable(
        "update_applied_receipt_write_failed",
        format!(
            "notegate-cli {version} was installed, but the install receipt could not be updated: {message}"
        ),
    )
}

fn safe_redirect_policy() -> Policy {
    Policy::custom(|attempt: Attempt<'_>| {
        if attempt.previous().len() >= 10 {
            return attempt.stop();
        }
        let next = attempt.url();
        if next.scheme() == "https" && is_trusted_update_host(next) {
            return attempt.follow();
        }
        if next.scheme() == "http" && is_loopback_host(next) {
            return attempt.follow();
        }
        attempt.stop()
    })
}

fn is_trusted_update_host(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    host == "github.com"
        || host == "objects.githubusercontent.com"
        || host == "release-assets.githubusercontent.com"
        || host == "github-releases.githubusercontent.com"
        || host.ends_with(".githubusercontent.com")
        || host.ends_with(".github.com")
}

fn smoke_check_candidate(path: &Path, version: &str) -> Result<(), CliError> {
    let output = Command::new(path)
        .arg("--version")
        .output()
        .map_err(|error| {
            let _ = fs::remove_file(path);
            CliError::protocol(
                "candidate_smoke_check_failed",
                format!("could not run the downloaded notegate-cli candidate: {error}"),
            )
        })?;
    if !output.status.success() {
        let _ = fs::remove_file(path);
        return Err(CliError::protocol(
            "candidate_smoke_check_failed",
            "the downloaded notegate-cli candidate did not run successfully",
        ));
    }
    let stdout = String::from_utf8(output.stdout).map_err(|_error| {
        let _ = fs::remove_file(path);
        CliError::protocol(
            "candidate_smoke_check_failed",
            "the downloaded notegate-cli candidate --version output was not UTF-8",
        )
    })?;
    if stdout != format!("notegate-cli {version}\n") {
        let _ = fs::remove_file(path);
        return Err(CliError::protocol(
            "candidate_version_mismatch",
            "the downloaded notegate-cli candidate version did not match the manifest",
        ));
    }
    Ok(())
}

fn replace_binary(temp_path: &Path, install_path: &Path) -> Result<(), CliError> {
    fs::rename(temp_path, install_path).map_err(|error| {
        let _ = fs::remove_file(temp_path);
        CliError::configuration(
            "binary_replace_failed",
            format!("could not replace {}: {error}", install_path.display()),
        )
    })
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), CliError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| {
            CliError::configuration(
                "directory_sync_failed",
                format!("could not sync {}: {error}", path.display()),
            )
        })
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), CliError> {
    Ok(())
}

fn lock_file(path: &Path) -> Result<RwLock<File>, CliError> {
    let parent = path.parent().ok_or_else(|| {
        CliError::configuration(
            "invalid_lock_path",
            "the notegate-cli update lock path has no parent directory",
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        CliError::configuration(
            "update_lock_unavailable",
            format!("could not create {}: {error}", parent.display()),
        )
    })?;
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path).map(RwLock::new).map_err(|error| {
        CliError::configuration(
            "update_lock_unavailable",
            format!("could not open {}: {error}", path.display()),
        )
    })
}

fn default_receipt_path(current_exe: &Path) -> PathBuf {
    current_exe
        .parent()
        .map(|parent| parent.join("notegate-cli-install-receipt.json"))
        .unwrap_or_else(|| PathBuf::from("notegate-cli-install-receipt.json"))
}

fn canonicalize_existing(path: &Path) -> Result<PathBuf, CliError> {
    fs::canonicalize(path).map_err(|_error| unmanaged_install())
}

fn current_target() -> Result<String, CliError> {
    let architecture = match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        _ => {
            return Err(CliError::configuration(
                "unsupported_update_target",
                "this CPU architecture is not supported by official notegate-cli updates",
            ));
        }
    };
    let os = match std::env::consts::OS {
        "linux" => "unknown-linux-gnu",
        "macos" => "apple-darwin",
        _ => {
            return Err(CliError::configuration(
                "unsupported_update_target",
                "this operating system is not supported by official notegate-cli updates",
            ));
        }
    };
    Ok(format!("{architecture}-{os}"))
}

fn validate_asset(asset: &ManifestAsset) -> Result<(), CliError> {
    if asset.name.is_empty()
        || asset.name.contains('/')
        || asset.name.contains('\\')
        || asset.name.contains("..")
    {
        return Err(CliError::protocol(
            "invalid_update_manifest",
            "the notegate-cli update manifest contained an invalid artifact name",
        ));
    }
    if asset.size == 0 || asset.size > ARTIFACT_MAX_BYTES {
        return Err(CliError::protocol(
            "invalid_update_manifest",
            "the notegate-cli update manifest contained an invalid artifact size",
        ));
    }
    if asset.sha256.len() != 64
        || !asset
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(CliError::protocol(
            "invalid_update_manifest",
            "the notegate-cli update manifest contained an invalid artifact checksum",
        ));
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

fn version_is_newer(candidate: &str, current: &str) -> Result<bool, CliError> {
    let candidate = parse_version(candidate)?;
    let current = parse_version(current)?;
    Ok(candidate > current)
}

fn validate_version(version: &str) -> Result<(), CliError> {
    parse_version(version).map(|_parsed| ())
}

fn parse_version(version: &str) -> Result<(u64, u64, u64), CliError> {
    let mut parts = version.split('.');
    let major = parse_version_part(parts.next(), version)?;
    let minor = parse_version_part(parts.next(), version)?;
    let patch = parse_version_part(parts.next(), version)?;
    if parts.next().is_some() {
        return Err(invalid_version(version));
    }
    Ok((major, minor, patch))
}

fn parse_version_part(part: Option<&str>, version: &str) -> Result<u64, CliError> {
    let Some(part) = part else {
        return Err(invalid_version(version));
    };
    if part.is_empty() || (part.len() > 1 && part.starts_with('0')) {
        return Err(invalid_version(version));
    }
    part.parse::<u64>()
        .map_err(|_error| invalid_version(version))
}

fn invalid_version(version: &str) -> CliError {
    CliError::protocol(
        "invalid_update_version",
        format!("the notegate-cli update version is invalid: {version}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Bytes;
    use axum::extract::State;
    use axum::routing::get;
    use std::error::Error;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::net::TcpListener;

    #[test]
    fn version_comparison_is_numeric() -> Result<(), CliError> {
        assert!(version_is_newer("0.1.78", "0.1.77")?);
        assert!(version_is_newer("0.10.0", "0.9.9")?);
        assert!(!version_is_newer("0.1.77", "0.1.77")?);
        assert!(!version_is_newer("0.1.76", "0.1.77")?);
        assert!(version_is_newer("01.1.0", "0.1.0").is_err());
        Ok(())
    }

    #[test]
    fn manifest_rejects_unsafe_assets() {
        let manifest = UpdateManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            version: "0.1.78".to_owned(),
            repository: DEFAULT_REPOSITORY.to_owned(),
            assets: BTreeMap::from([(
                "aarch64-apple-darwin".to_owned(),
                ManifestAsset {
                    name: "../notegate-cli".to_owned(),
                    sha256: "a".repeat(64),
                    size: 1,
                },
            )]),
        };
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn receipt_must_match_path_target_and_repository() -> Result<(), Box<dyn Error>> {
        let dir = test_dir("receipt")?;
        let install_path = dir.join("notegate-cli");
        fs::write(&install_path, b"binary")?;
        let target = current_target().map_err(|_error| {
            std::io::Error::other("current test target must support notegate-cli updates")
        })?;
        let receipt = InstallReceipt {
            schema_version: RECEIPT_SCHEMA_VERSION,
            managed_by: RECEIPT_MANAGED_BY.to_owned(),
            repository: DEFAULT_REPOSITORY.to_owned(),
            install_path: install_path.to_string_lossy().to_string(),
            target: target.clone(),
            installed_version: env!("CARGO_PKG_VERSION").to_owned(),
        };
        validate_receipt(&receipt, &install_path, &target)
            .map_err(|_error| std::io::Error::other("receipt should be valid"))?;

        let wrong_target = InstallReceipt {
            target: "x86_64-unknown-linux-gnu-wrong".to_owned(),
            ..receipt
        };
        assert!(validate_receipt(&wrong_target, &install_path, &target).is_err());
        let _ = fs::remove_dir_all(dir);
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn update_replaces_managed_binary_after_checksum_verification()
    -> Result<(), Box<dyn Error>> {
        let dir = test_dir("replace")?;
        let install_path = dir.join("notegate-cli");
        let old_script = "#!/bin/sh\necho 'notegate-cli 0.1.77'\n";
        fs::write(&install_path, old_script)?;
        make_executable(&install_path)?;
        let receipt_path = dir.join("notegate-cli-install-receipt.json");
        let target = current_target().map_err(|_error| {
            std::io::Error::other("current test target must support notegate-cli updates")
        })?;
        write_receipt(
            &receipt_path,
            &InstallReceipt {
                schema_version: RECEIPT_SCHEMA_VERSION,
                managed_by: RECEIPT_MANAGED_BY.to_owned(),
                repository: DEFAULT_REPOSITORY.to_owned(),
                install_path: install_path.to_string_lossy().to_string(),
                target: target.clone(),
                installed_version: env!("CARGO_PKG_VERSION").to_owned(),
            },
        )
        .map_err(|_error| std::io::Error::other("receipt should be writable"))?;

        let new_script = b"#!/bin/sh\necho 'notegate-cli 9.9.9'\n".to_vec();
        let artifact_name = format!("notegate-cli-{target}");
        let manifest = UpdateManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            version: "9.9.9".to_owned(),
            repository: DEFAULT_REPOSITORY.to_owned(),
            assets: BTreeMap::from([(
                target,
                ManifestAsset {
                    name: artifact_name.clone(),
                    sha256: sha256_hex(&new_script),
                    size: new_script.len(),
                },
            )]),
        };
        let manifest_bytes = serde_json::to_vec(&manifest)?;
        let server = spawn_test_server(artifact_name, new_script, manifest_bytes).await?;

        let result = run_with_settings(UpdateSettings {
            current_exe: install_path.clone(),
            receipt_path,
            manifest_url: format!("{server}/notegate-cli-manifest.json"),
            asset_base_url: Some(format!("{server}/")),
            check_only: false,
            timeout: Duration::from_secs(5),
        })
        .await
        .map_err(|_error| std::io::Error::other("update should succeed"))?;
        assert_eq!(
            result.get("status").and_then(Value::as_str),
            Some("updated")
        );
        let output = Command::new(&install_path).arg("--version").output()?;
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout)?,
            "notegate-cli 9.9.9\n".to_owned()
        );
        let _ = fs::remove_dir_all(dir);
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn checksum_mismatch_leaves_old_binary_unchanged() -> Result<(), Box<dyn Error>> {
        let dir = test_dir("checksum")?;
        let (install_path, receipt_path, target) = managed_script_install(&dir)?;
        let new_script = b"#!/bin/sh\necho 'notegate-cli 9.9.9'\n".to_vec();
        let artifact_name = format!("notegate-cli-{target}");
        let manifest = manifest_bytes(
            &target,
            "9.9.9",
            &artifact_name,
            "0".repeat(64),
            new_script.len(),
        )?;
        let server = spawn_test_server(artifact_name, new_script, manifest).await?;

        let error = run_with_settings(UpdateSettings {
            current_exe: install_path.clone(),
            receipt_path,
            manifest_url: format!("{server}/notegate-cli-manifest.json"),
            asset_base_url: Some(format!("{server}/")),
            check_only: false,
            timeout: Duration::from_secs(5),
        })
        .await
        .err()
        .ok_or_else(|| std::io::Error::other("checksum mismatch should fail"))?;

        assert_eq!(
            error.body().get("error").and_then(Value::as_str),
            Some("update_checksum_mismatch")
        );
        assert_eq!(
            candidate_version_output(&install_path)?,
            "notegate-cli 0.1.77\n"
        );
        let _ = fs::remove_dir_all(dir);
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn candidate_version_mismatch_leaves_old_binary_unchanged() -> Result<(), Box<dyn Error>>
    {
        let dir = test_dir("candidate-version")?;
        let (install_path, receipt_path, target) = managed_script_install(&dir)?;
        let wrong_script = b"#!/bin/sh\necho 'notegate-cli 8.8.8'\n".to_vec();
        let artifact_name = format!("notegate-cli-{target}");
        let manifest = manifest_bytes(
            &target,
            "9.9.9",
            &artifact_name,
            sha256_hex(&wrong_script),
            wrong_script.len(),
        )?;
        let server = spawn_test_server(artifact_name, wrong_script, manifest).await?;

        let error = run_with_settings(UpdateSettings {
            current_exe: install_path.clone(),
            receipt_path,
            manifest_url: format!("{server}/notegate-cli-manifest.json"),
            asset_base_url: Some(format!("{server}/")),
            check_only: false,
            timeout: Duration::from_secs(5),
        })
        .await
        .err()
        .ok_or_else(|| std::io::Error::other("candidate mismatch should fail"))?;

        assert_eq!(
            error.body().get("error").and_then(Value::as_str),
            Some("candidate_version_mismatch")
        );
        assert_eq!(
            candidate_version_output(&install_path)?,
            "notegate-cli 0.1.77\n"
        );
        let _ = fs::remove_dir_all(dir);
        Ok(())
    }

    #[tokio::test]
    async fn download_bound_rejects_streams_without_trusting_content_length()
    -> Result<(), Box<dyn Error>> {
        let server = spawn_test_server(
            "notegate-cli-test".to_owned(),
            b"12345".to_vec(),
            b"{}".to_vec(),
        )
        .await?;
        let client = reqwest::Client::builder()
            .redirect(safe_redirect_policy())
            .build()?;
        let url = safe_download_url(&format!("{server}/notegate-cli-test"), "test URL")?;

        let error = download_bounded(&client, url, 4)
            .await
            .err()
            .ok_or_else(|| std::io::Error::other("oversized stream should fail"))?;

        assert_eq!(
            error.body().get("error").and_then(Value::as_str),
            Some("update_download_too_large")
        );
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn lock_contention_returns_structured_retryable_error() -> Result<(), Box<dyn Error>> {
        let dir = test_dir("lock")?;
        let (install_path, receipt_path, target) = managed_script_install(&dir)?;
        let new_script = b"#!/bin/sh\necho 'notegate-cli 9.9.9'\n".to_vec();
        let artifact_name = format!("notegate-cli-{target}");
        let manifest = manifest_bytes(
            &target,
            "9.9.9",
            &artifact_name,
            sha256_hex(&new_script),
            new_script.len(),
        )?;
        let server = spawn_test_server(artifact_name, new_script, manifest).await?;
        let lock_path = dir.join(".notegate-cli-update.lock");
        let mut lock = lock_file(&lock_path)?;
        let _guard = lock.write()?;

        let error = run_with_settings(UpdateSettings {
            current_exe: install_path.clone(),
            receipt_path,
            manifest_url: format!("{server}/notegate-cli-manifest.json"),
            asset_base_url: Some(format!("{server}/")),
            check_only: false,
            timeout: Duration::from_secs(5),
        })
        .await
        .err()
        .ok_or_else(|| std::io::Error::other("lock contention should fail"))?;

        assert_eq!(error.exit_code(), crate::error::EXIT_UNAVAILABLE);
        assert_eq!(
            error.body().get("error").and_then(Value::as_str),
            Some("update_in_progress")
        );
        assert_eq!(
            error
                .body()
                .pointer("/data/retryable")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            candidate_version_output(&install_path)?,
            "notegate-cli 0.1.77\n"
        );
        let _ = fs::remove_dir_all(dir);
        Ok(())
    }

    #[cfg(unix)]
    fn managed_script_install(dir: &Path) -> Result<(PathBuf, PathBuf, String), Box<dyn Error>> {
        let install_path = dir.join("notegate-cli");
        fs::write(&install_path, "#!/bin/sh\necho 'notegate-cli 0.1.77'\n")?;
        make_executable(&install_path)?;
        let receipt_path = dir.join("notegate-cli-install-receipt.json");
        let target = current_target().map_err(|_error| {
            std::io::Error::other("current test target must support notegate-cli updates")
        })?;
        write_receipt(
            &receipt_path,
            &InstallReceipt {
                schema_version: RECEIPT_SCHEMA_VERSION,
                managed_by: RECEIPT_MANAGED_BY.to_owned(),
                repository: DEFAULT_REPOSITORY.to_owned(),
                install_path: install_path.to_string_lossy().to_string(),
                target: target.clone(),
                installed_version: env!("CARGO_PKG_VERSION").to_owned(),
            },
        )?;
        Ok((install_path, receipt_path, target))
    }

    fn manifest_bytes(
        target: &str,
        version: &str,
        name: &str,
        sha256: String,
        size: usize,
    ) -> Result<Vec<u8>, Box<dyn Error>> {
        let manifest = UpdateManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            version: version.to_owned(),
            repository: DEFAULT_REPOSITORY.to_owned(),
            assets: BTreeMap::from([(
                target.to_owned(),
                ManifestAsset {
                    name: name.to_owned(),
                    sha256,
                    size,
                },
            )]),
        };
        serde_json::to_vec(&manifest).map_err(Into::into)
    }

    fn candidate_version_output(path: &Path) -> Result<String, Box<dyn Error>> {
        let output = Command::new(path).arg("--version").output()?;
        if !output.status.success() {
            return Err(std::io::Error::other("candidate --version failed").into());
        }
        String::from_utf8(output.stdout).map_err(Into::into)
    }

    #[cfg(unix)]
    fn make_executable(path: &Path) -> Result<(), Box<dyn Error>> {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
        Ok(())
    }

    async fn spawn_test_server(
        artifact_name: String,
        artifact: Vec<u8>,
        manifest: Vec<u8>,
    ) -> Result<String, Box<dyn Error>> {
        #[derive(Clone)]
        struct TestState {
            artifact_name: String,
            artifact: Arc<Vec<u8>>,
            manifest: Arc<Vec<u8>>,
        }
        async fn serve_manifest(State(state): State<TestState>) -> Bytes {
            Bytes::from((*state.manifest).clone())
        }
        async fn serve_artifact(State(state): State<TestState>) -> Bytes {
            let _ = &state.artifact_name;
            Bytes::from((*state.artifact).clone())
        }
        let state = TestState {
            artifact_name: artifact_name.clone(),
            artifact: Arc::new(artifact),
            manifest: Arc::new(manifest),
        };
        let app = Router::new()
            .route("/notegate-cli-manifest.json", get(serve_manifest))
            .route(&format!("/{artifact_name}"), get(serve_artifact))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(format!("http://{address}"))
    }

    fn test_dir(label: &str) -> Result<PathBuf, Box<dyn Error>> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "notegate-cli-update-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path)?;
        Ok(path)
    }
}
