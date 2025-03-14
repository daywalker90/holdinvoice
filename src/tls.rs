//! Utilities to manage TLS certificates.
use anyhow::{Context, Result};
use log::debug;
use rcgen::{Certificate, KeyPair};
use std::path::Path;

/// Just a wrapper around a certificate and an associated keypair.
#[derive(Clone, Debug)]
pub(crate) struct Identity {
    key: Vec<u8>,
    certificate: Vec<u8>,
}

impl Identity {
    fn to_certificate(&self) -> Result<Certificate> {
        let keystr = String::from_utf8_lossy(&self.key);
        let key = KeyPair::from_pem(&keystr)?;
        let certstr = String::from_utf8_lossy(&self.certificate);
        let params = rcgen::CertificateParams::from_ca_cert_pem(&certstr)?;
        let cert = params.self_signed(&key)?;
        Ok(cert)
    }

    fn to_keypair(&self) -> Result<KeyPair> {
        let keystr = String::from_utf8_lossy(&self.key);
        let key = KeyPair::from_pem(&keystr)?;
        Ok(key)
    }

    pub fn to_tonic_identity(&self) -> tonic::transport::Identity {
        tonic::transport::Identity::from_pem(&self.certificate, &self.key)
    }
}

/// Ensure that we have a certificate authority, and child keypairs
/// and certificates for the server and the client. It'll generate
/// them in the provided `directory`. The following files are
/// included:
///
/// - `ca.pem`: The self-signed certificate of the CA
/// - `ca-key.pem`: The key used by the CA to sign certificates
/// - `server.pem`: The server certificate, signed by the CA
/// - `server-key.pem`: The server private key
/// - `client.pem`: The client certificate, signed by the CA
/// - `client-key.pem`: The client private key
///
/// The `grpc-plugin` will use the `server.pem` certificate, while a
/// client is supposed to use the `client.pem` and associated
/// keys. Notice that this isn't strictly necessary since the server
/// will accept any client that is signed by the CA. In future we
/// might add runes, making the distinction more important.
///
/// Returns the server identity and the root CA certificate.
pub(crate) fn init(directory: &Path) -> Result<(Identity, Vec<u8>)> {
    let ca = generate_or_load_identity("cln Root CA", directory, "ca", None)?;
    let server = generate_or_load_identity("cln grpc Server", directory, "server", Some(&ca))?;
    let _client = generate_or_load_identity("cln grpc Client", directory, "client", Some(&ca))?;
    Ok((server, ca.certificate))
}

/// Generate a given identity
fn generate_or_load_identity(
    name: &str,
    directory: &Path,
    filename: &str,
    parent: Option<&Identity>,
) -> Result<Identity> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    // Just our naming convention here.
    let cert_path = directory.join(format!("{}.pem", filename));
    let key_path = directory.join(format!("{}-key.pem", filename));
    // Did we have to generate a new key? In that case we also need to
    // regenerate the certificate
    if !key_path.exists() || !cert_path.exists() {
        debug!(
            "Generating a new keypair in {:?}, it didn't exist",
            &key_path
        );
        let keypair = KeyPair::generate()?;

        // Create the file, but make it user-readable only:
        let mut file = std::fs::File::create(&key_path)?;
        let mut perms = std::fs::metadata(&key_path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&key_path, perms)?;

        // Only after changing the permissions we can write the
        // private key
        file.write_all(keypair.serialize_pem().as_bytes())?;
        drop(file);

        debug!(
            "Generating a new certificate for key {:?} at {:?}",
            &key_path, &cert_path
        );

        // Configure the certificate we want.
        let subject_alt_names = vec!["cln".to_owned(), "localhost".to_owned()];
        let mut params = rcgen::CertificateParams::new(subject_alt_names)?;
        if parent.is_none() {
            params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        } else {
            params.is_ca = rcgen::IsCa::NoCa;
        }
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, name);

        std::fs::write(
            &cert_path,
            match parent {
                None => params.self_signed(&keypair)?.pem(),
                Some(ca) => params
                    .signed_by(&keypair, &ca.to_certificate()?, &ca.to_keypair()?)?
                    .pem(),
            },
        )
        .context("writing certificate to file")?;
    }

    let key = std::fs::read(&key_path)?;
    let certificate = std::fs::read(cert_path)?;
    Ok(Identity { certificate, key })
}

pub fn do_certificates_exist(cert_dir: &Path) -> bool {
    let required_files = [
        "server.pem",
        "server-key.pem",
        "client.pem",
        "client-key.pem",
        "ca.pem",
        "ca-key.pem",
    ];

    required_files.iter().all(|file| {
        let path = cert_dir.join(file);
        path.exists() && path.metadata().map(|m| m.len() > 0).unwrap_or(false)
    })
}
