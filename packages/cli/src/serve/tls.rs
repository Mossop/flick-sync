//! TLS certificate loading, with automatic reloading when the certificate or
//! private key files change, e.g. when a certificate is renewed.

use std::{
    fs, result,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime},
};

use anyhow::{Context, anyhow, bail};
use rustls::{
    crypto::CryptoProvider,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
};
use tokio::time::sleep;
use tracing::{info, warn};

use crate::Result;

/// How often to check the certificate and private key files for changes.
const RELOAD_CHECK_INTERVAL: Duration = Duration::from_secs(30);

fn is_pem(data: &[u8]) -> bool {
    data.windows(10).any(|w| w == b"-----BEGIN")
}

/// Loads a certificate chain in PEM format, or a single certificate in DER
/// format.
fn load_certificates(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let data = fs::read(path).with_context(|| format!("Failed to read certificate {path}"))?;

    if !is_pem(&data) {
        return Ok(vec![CertificateDer::from(data)]);
    }

    let certs = CertificateDer::pem_slice_iter(&data)
        .collect::<result::Result<Vec<_>, _>>()
        .map_err(|e| anyhow!("Failed to parse certificate {path}: {e}"))?;

    if certs.is_empty() {
        bail!("No certificates found in {path}");
    }

    Ok(certs)
}

/// Loads a private key in PEM or DER format.
fn load_private_key(path: &str) -> Result<PrivateKeyDer<'static>> {
    let data = fs::read(path).with_context(|| format!("Failed to read private key {path}"))?;

    if is_pem(&data) {
        PrivateKeyDer::from_pem_slice(&data)
            .map_err(|e| anyhow!("Failed to parse private key {path}: {e}"))
    } else {
        PrivateKeyDer::try_from(data)
            .map_err(|e| anyhow!("Failed to parse private key {path}: {e}"))
    }
}

/// Loads the certificate and private key, checking that they match.
fn load_certified_key(cert_file: &str, key_file: &str) -> Result<CertifiedKey> {
    let certs = load_certificates(cert_file)?;
    let private_key = load_private_key(key_file)?;

    let provider =
        CryptoProvider::get_default().context("No TLS crypto provider has been installed")?;

    CertifiedKey::from_der(certs, private_key, provider)
        .context("Invalid TLS certificate or private key")
}

/// Identifies the current version of a file. Following symlinks means a
/// symlink being switched to a new file is noticed too.
type FileVersion = Option<(SystemTime, u64)>;

fn file_version(path: &str) -> FileVersion {
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

/// Provides the TLS certificate for connections, reloading it when the files
/// change.
#[derive(Debug)]
pub(super) struct ReloadingCertResolver {
    cert_file: String,
    key_file: String,
    current: RwLock<Arc<CertifiedKey>>,
}

impl ReloadingCertResolver {
    pub(super) fn new(cert_file: &str, key_file: &str) -> Result<Self> {
        Ok(Self {
            cert_file: cert_file.to_owned(),
            key_file: key_file.to_owned(),
            current: RwLock::new(Arc::new(load_certified_key(cert_file, key_file)?)),
        })
    }

    fn versions(&self) -> (FileVersion, FileVersion) {
        (file_version(&self.cert_file), file_version(&self.key_file))
    }

    /// Checks the files periodically and reloads the certificate when they
    /// change. If the new files can't be loaded the current certificate stays
    /// in use.
    pub(super) async fn watch(self: Arc<Self>) {
        let mut last_versions = self.versions();

        loop {
            sleep(RELOAD_CHECK_INTERVAL).await;

            let versions = self.versions();
            if versions == last_versions {
                continue;
            }
            // Only retry after a further change, e.g. a renewal that writes
            // the certificate and key one after the other.
            last_versions = versions;

            match load_certified_key(&self.cert_file, &self.key_file) {
                Ok(certified_key) => {
                    *self.current.write().unwrap() = Arc::new(certified_key);
                    info!(cert_file = self.cert_file, "Reloaded TLS certificate");
                }
                Err(e) => {
                    warn!(error = %format_args!("{e:#}"), "Failed to reload TLS certificate, still using the previous one");
                }
            }
        }
    }
}

impl ResolvesServerCert for ReloadingCertResolver {
    fn resolve(&self, _client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(self.current.read().unwrap().clone())
    }
}
