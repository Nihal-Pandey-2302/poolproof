//! TLS for both kinds of client: tokio-postgres through tokio-postgres-rustls, and
//! raw `send` connections through tokio-rustls. `sslmode` follows libpq.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};

/// libpq's `sslmode`. `allow` is missing: it tries plaintext first, which no pooler
/// test needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslMode {
    Disable,
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

impl SslMode {
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "disable" => SslMode::Disable,
            "prefer" => SslMode::Prefer,
            "require" => SslMode::Require,
            "verify-ca" => SslMode::VerifyCa,
            "verify-full" => SslMode::VerifyFull,
            other => bail!("unsupported sslmode `{other}`"),
        })
    }

    /// The mode tokio-postgres knows: it only decides whether to ask for TLS and
    /// whether plaintext is acceptable. Verification is ours.
    pub fn driver_mode(self) -> tokio_postgres::config::SslMode {
        use tokio_postgres::config::SslMode as M;
        match self {
            SslMode::Disable => M::Disable,
            SslMode::Prefer => M::Prefer,
            SslMode::Require | SslMode::VerifyCa | SslMode::VerifyFull => M::Require,
        }
    }
}

impl fmt::Display for SslMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SslMode::Disable => "disable",
            SslMode::Prefer => "prefer",
            SslMode::Require => "require",
            SslMode::VerifyCa => "verify-ca",
            SslMode::VerifyFull => "verify-full",
        })
    }
}

/// Where trusted root certificates come from (libpq's `sslrootcert`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootCerts {
    File(PathBuf),
    /// The operating system's store (`sslrootcert=system`).
    System,
}

/// How an endpoint uses TLS: the settings, and the rustls config built from them.
#[derive(Clone)]
pub struct Tls {
    pub mode: SslMode,
    pub roots: Option<RootCerts>,
    /// `None` exactly when `mode` is `Disable`.
    pub config: Option<Arc<ClientConfig>>,
}

impl PartialEq for Tls {
    fn eq(&self, other: &Self) -> bool {
        // The config is built from the other two.
        self.mode == other.mode && self.roots == other.roots
    }
}

impl Eq for Tls {}

impl fmt::Debug for Tls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tls")
            .field("mode", &self.mode)
            .field("roots", &self.roots)
            .finish()
    }
}

impl Tls {
    /// Builds the rustls config the way libpq checks certificates:
    /// - prefer: no check.
    /// - require: no check, unless a root certificate is given or the default one
    ///   exists, in which case it acts as verify-ca.
    /// - verify-ca: the chain must lead to a trusted root; the name isn't checked.
    /// - verify-full: chain and host name.
    ///
    /// `default_root` is libpq's `~/.postgresql/root.crt`.
    pub fn new(mode: SslMode, roots: Option<RootCerts>, default_root: Option<PathBuf>) -> Result<Self> {
        if mode == SslMode::Disable {
            return Ok(Tls {
                mode,
                roots,
                config: None,
            });
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let roots = roots.or_else(|| match mode {
            SslMode::Prefer => None,
            _ => default_root.filter(|p| p.is_file()).map(RootCerts::File),
        });
        let check = match (mode, &roots) {
            (SslMode::Disable | SslMode::Prefer, _) | (SslMode::Require, None) => Check::None,
            (SslMode::Require | SslMode::VerifyCa, Some(_)) => Check::Chain,
            (SslMode::VerifyFull, Some(_)) => Check::Full,
            (_, None) => {
                bail!("sslmode={mode} needs a root certificate: set sslrootcert, or put one in ~/.postgresql/root.crt")
            }
        };
        let verifier: Arc<dyn ServerCertVerifier> = match (check, &roots) {
            (Check::None, _) | (_, None) => Arc::new(NoCheck(provider.clone())),
            (check, Some(roots)) => {
                let store = Arc::new(root_store(roots)?);
                let webpki = WebPkiServerVerifier::builder_with_provider(store, provider.clone())
                    .build()
                    .context("building the certificate verifier")?;
                match check {
                    Check::Full => webpki,
                    _ => Arc::new(ChainOnly(webpki)),
                }
            }
        };
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .context("TLS protocol versions")?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        Ok(Tls {
            mode,
            roots,
            config: Some(Arc::new(config)),
        })
    }
}

#[derive(Clone, Copy)]
enum Check {
    None,
    Chain,
    Full,
}

fn root_store(roots: &RootCerts) -> Result<RootCertStore> {
    let mut store = RootCertStore::empty();
    match roots {
        RootCerts::File(path) => {
            let certs = CertificateDer::pem_file_iter(path)
                .and_then(|certs| certs.collect::<Result<Vec<_>, _>>())
                .with_context(|| format!("reading root certificates from {}", path.display()))?;
            let (added, _) = store.add_parsable_certificates(certs);
            if added == 0 {
                bail!("no usable root certificate in {}", path.display());
            }
        }
        RootCerts::System => {
            let found = rustls_native_certs::load_native_certs();
            let (added, _) = store.add_parsable_certificates(found.certs);
            if added == 0 {
                bail!("no root certificates in the system store");
            }
        }
    }
    Ok(store)
}

/// libpq's default root certificate: ~/.postgresql/root.crt, or
/// %APPDATA%\postgresql\root.crt on Windows.
pub fn default_root(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if cfg!(windows) {
        env("APPDATA").map(|dir| Path::new(&dir).join("postgresql").join("root.crt"))
    } else {
        env("HOME").map(|dir| Path::new(&dir).join(".postgresql").join("root.crt"))
    }
}

/// Accepts any certificate, but still checks the handshake signatures against it, so
/// the session is encrypted to whoever holds that certificate's key.
#[derive(Debug)]
struct NoCheck(Arc<CryptoProvider>);

impl ServerCertVerifier for NoCheck {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// verify-ca: the full check, except that a certificate for another name passes.
/// The name check comes after the chain check, so a name error means the chain is good.
#[derive(Debug)]
struct ChainOnly(Arc<WebPkiServerVerifier>);

impl ServerCertVerifier for ChainOnly {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match self
            .0
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
        {
            Err(rustls::Error::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
            )) => Ok(ServerCertVerified::assertion()),
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_verify_schemes()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
    use rustls::ServerConfig;
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;

    use super::*;
    use crate::runner::Timeouts;
    use crate::runner::pg::{cancel, connect};
    use crate::runner::raw::RawConn;
    use crate::targets::Endpoint;

    const SSL_REQUEST: u32 = 80877103;
    const CANCEL_REQUEST: u32 = 80877102;

    /// A CA (its PEM written to a file) and a server certificate it signed for `localhost`.
    struct Pki {
        ca_file: PathBuf,
        server: Arc<ServerConfig>,
    }

    fn pki(tag: &str) -> Pki {
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate().unwrap()).unwrap();
        let key = KeyPair::generate().unwrap();
        let cert = CertificateParams::new(vec!["localhost".to_string()])
            .unwrap()
            .signed_by(&key, &ca)
            .unwrap();
        let ca_file = std::env::temp_dir().join(format!("poolproof-tls-{}-{tag}.pem", std::process::id()));
        std::fs::write(&ca_file, ca.pem()).unwrap();
        let server = ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
            )
            .unwrap();
        Pki {
            ca_file,
            server: Arc::new(server),
        }
    }

    type Events = Arc<Mutex<Vec<String>>>;

    async fn read_message<S: AsyncRead + Unpin>(s: &mut S) -> Option<Vec<u8>> {
        let len = s.read_u32().await.ok()? as usize;
        let mut body = vec![0; len.checked_sub(4)?];
        s.read_exact(&mut body).await.ok()?;
        Some(body)
    }

    fn code(body: &[u8]) -> u32 {
        u32::from_be_bytes(body[..4].try_into().unwrap())
    }

    /// Answers a startup with AuthenticationOk, BackendKeyData and ReadyForQuery, then
    /// waits for the client to leave. Records what it saw as `<how> <what>`.
    async fn serve<S: AsyncRead + AsyncWrite + Unpin>(s: &mut S, first: Vec<u8>, how: &str, events: &Events) {
        if code(&first) == CANCEL_REQUEST {
            events.lock().unwrap().push(format!("{how} cancel"));
            return;
        }
        events.lock().unwrap().push(format!("{how} startup"));
        let reply = [
            &b"R\0\0\0\x08\0\0\0\0"[..],
            b"K\0\0\0\x0c\0\0\0\x2a\0\0\0\x07",
            b"Z\0\0\0\x05I",
        ]
        .concat();
        let _ = s.write_all(&reply).await;
        while read_message(s).await.is_some() {}
    }

    /// A Postgres stand-in on a local port. With `offer_tls` it answers SSLRequest with
    /// `S` and the certificate from `pki`, else with `N`.
    async fn server(pki: &Pki, offer_tls: bool) -> (u16, Events) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let events = Events::default();
        let acceptor = TlsAcceptor::from(pki.server.clone());
        let log = events.clone();
        tokio::spawn(async move {
            while let Ok((mut tcp, _)) = listener.accept().await {
                let (acceptor, events) = (acceptor.clone(), log.clone());
                tokio::spawn(async move {
                    let Some(first) = read_message(&mut tcp).await else {
                        return;
                    };
                    if code(&first) != SSL_REQUEST {
                        return serve(&mut tcp, first, "plain", &events).await;
                    }
                    if !offer_tls {
                        tcp.write_all(b"N").await.unwrap();
                        let Some(next) = read_message(&mut tcp).await else {
                            return;
                        };
                        return serve(&mut tcp, next, "plain", &events).await;
                    }
                    tcp.write_all(b"S").await.unwrap();
                    let Ok(mut tls) = acceptor.accept(tcp).await else {
                        events.lock().unwrap().push("handshake failed".into());
                        return;
                    };
                    let Some(next) = read_message(&mut tls).await else {
                        return;
                    };
                    serve(&mut tls, next, "tls", &events).await;
                });
            }
        });
        (port, events)
    }

    fn endpoint(host: &str, port: u16, params: &str) -> Endpoint {
        Endpoint::parse(&format!("postgres://u@{host}:{port}/db?{params}")).unwrap()
    }

    /// Connects with a raw client and a tokio-postgres client; the error of either.
    async fn connect_both(e: &Endpoint) -> (Result<()>, Result<()>) {
        let raw = RawConn::connect(e, Duration::from_secs(5)).await.map(|_| ());
        let pg = connect(e, &Timeouts::default()).await.map(|_| ());
        (raw, pg)
    }

    #[tokio::test]
    async fn sslmodes_check_certificates_like_libpq() {
        let other = pki("other");
        let pki = pki("modes");
        let ca = pki.ca_file.display();
        let wrong_ca = other.ca_file.display();
        let (port, _) = server(&pki, true).await;
        let ok = |host: &str, params: String| (host.to_string(), params, true);
        let fails = |host: &str, params: String| (host.to_string(), params, false);
        for (host, params, works) in [
            ok("localhost", format!("sslmode=verify-full&sslrootcert={ca}")),
            // The certificate is for localhost, not 127.0.0.1.
            fails("127.0.0.1", format!("sslmode=verify-full&sslrootcert={ca}")),
            ok("127.0.0.1", format!("sslmode=verify-ca&sslrootcert={ca}")),
            fails("localhost", format!("sslmode=verify-ca&sslrootcert={wrong_ca}")),
            ok("127.0.0.1", "sslmode=require".to_string()),
            // With a root certificate, require checks the chain like verify-ca.
            fails("127.0.0.1", format!("sslmode=require&sslrootcert={wrong_ca}")),
            ok("127.0.0.1", "sslmode=prefer".to_string()),
        ] {
            let (raw, pg) = connect_both(&endpoint(&host, port, &params)).await;
            assert_eq!(raw.is_ok(), works, "raw {host} {params}: {raw:?}");
            assert_eq!(pg.is_ok(), works, "pg {host} {params}: {pg:?}");
        }
    }

    #[tokio::test]
    async fn plaintext_only_when_allowed() {
        let pki = pki("plain");
        let (port, events) = server(&pki, false).await;
        let (raw, pg) = connect_both(&endpoint("127.0.0.1", port, "sslmode=require")).await;
        assert!(format!("{:#}", raw.unwrap_err()).contains("does not support TLS"));
        assert!(pg.is_err());
        assert!(events.lock().unwrap().is_empty(), "no startup in plaintext");

        let (raw, pg) = connect_both(&endpoint("127.0.0.1", port, "sslmode=prefer")).await;
        raw.unwrap();
        pg.unwrap();
        assert_eq!(*events.lock().unwrap(), ["plain startup", "plain startup"]);

        // disable doesn't even ask.
        let (tls_port, events) = server(&pki, true).await;
        let (raw, pg) = connect_both(&endpoint("localhost", tls_port, "sslmode=disable")).await;
        raw.unwrap();
        pg.unwrap();
        assert_eq!(*events.lock().unwrap(), ["plain startup", "plain startup"]);
    }

    #[tokio::test]
    async fn cancel_request_goes_over_tls() {
        let pki = pki("cancel");
        let (port, events) = server(&pki, true).await;
        let e = endpoint(
            "localhost",
            port,
            &format!("sslmode=verify-full&sslrootcert={}", pki.ca_file.display()),
        );
        let (client, _driver, _) = connect(&e, &Timeouts::default()).await.unwrap();
        cancel(&client.cancel_token(), &e).await.unwrap();
        // The server records the request after the client is done sending it.
        for _ in 0..100 {
            if events.lock().unwrap().len() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(*events.lock().unwrap(), ["tls startup", "tls cancel"]);
    }

    #[test]
    fn verify_modes_need_a_root_certificate() {
        let err = Tls::new(SslMode::VerifyFull, None, None).err().unwrap();
        assert!(err.to_string().contains("needs a root certificate"), "{err}");
        assert!(Tls::new(SslMode::Require, None, Some("/nonexistent".into())).is_ok());
    }
}
