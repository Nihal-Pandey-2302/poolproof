//! TLS for both kinds of client: tokio-postgres through tokio-postgres-rustls, and
//! raw `send` connections through tokio-rustls. `sslmode` follows libpq.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

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

/// Whether a TLS error means the server asked for a client certificate, which poolproof
/// doesn't send yet.
pub fn wants_client_certificate(err: &(dyn std::error::Error + 'static)) -> bool {
    let mut cause = Some(err);
    while let Some(e) = cause {
        let rustls_error = e.downcast_ref::<rustls::Error>().or_else(|| {
            e.downcast_ref::<std::io::Error>()
                .and_then(|io| io.get_ref())
                .and_then(|inner| inner.downcast_ref::<rustls::Error>())
        });
        if let Some(rustls::Error::AlertReceived(rustls::AlertDescription::CertificateRequired)) = rustls_error {
            return true;
        }
        cause = e.source();
    }
    false
}

/// Where trusted root certificates come from (libpq's `sslrootcert`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootCerts {
    File(PathBuf),
    /// The operating system's store (`sslrootcert=system`).
    System,
}

/// How an endpoint uses TLS: the settings, and the rustls config built from them on
/// first use.
#[derive(Clone)]
pub struct Tls {
    pub mode: SslMode,
    pub roots: Option<RootCerts>,
    check: Check,
    config: Arc<OnceLock<Arc<ClientConfig>>>,
}

impl PartialEq for Tls {
    fn eq(&self, other: &Self) -> bool {
        // The rest follows from these two.
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
    /// Checks the settings, and decides how certificates are checked, as libpq does:
    /// - prefer: no check.
    /// - require: no check, unless a root certificate is given or the default one
    ///   exists, in which case it acts as verify-ca.
    /// - verify-ca: the chain must lead to a trusted root; the name isn't checked.
    /// - verify-full: chain and host name.
    ///
    /// `default_root` is libpq's `~/.postgresql/root.crt`.
    pub fn new(mode: SslMode, roots: Option<RootCerts>, default_root: Option<PathBuf>) -> Result<Self> {
        // As in libpq 16+: the system store trusts every public CA, so anything short of
        // a host name check would accept any public certificate.
        if roots == Some(RootCerts::System) && mode != SslMode::VerifyFull {
            bail!("weak sslmode \"{mode}\" may not be used with sslrootcert=system (use \"verify-full\")");
        }
        let roots = roots.or_else(|| match mode {
            SslMode::Disable => None,
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
        Ok(Tls {
            mode,
            roots,
            check,
            config: Arc::default(),
        })
    }

    /// The rustls config, or `None` without TLS. The root certificates are read here,
    /// on first use, so a missing or unreadable file is an error for this endpoint's
    /// connections only, not for loading every target.
    /// Errors carry the `TlsSetup` context: retrying won't help.
    pub fn client_config(&self) -> Result<Option<Arc<ClientConfig>>> {
        if self.mode == SslMode::Disable {
            return Ok(None);
        }
        if let Some(config) = self.config.get() {
            return Ok(Some(config.clone()));
        }
        let config = self.build().context(TlsSetup)?;
        // Another connection may have built it meanwhile; either is the same.
        Ok(Some(self.config.get_or_init(|| Arc::new(config)).clone()))
    }

    fn build(&self) -> Result<ClientConfig> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier: Arc<dyn ServerCertVerifier> = match (self.check, &self.roots) {
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
        Ok(config)
    }
}

/// Marks an error building the TLS config, e.g. a missing CA file: the same on every
/// try, so callers that retry connecting can stop at once.
#[derive(Debug)]
pub struct TlsSetup;

impl fmt::Display for TlsSetup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TLS setup failed")
    }
}

#[derive(Clone, Copy, Debug)]
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
        /// The same certificate, but the server insists on a client certificate.
        server_wanting_client_cert: Arc<ServerConfig>,
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
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let builder = || {
            ServerConfig::builder_with_provider(provider.clone())
                .with_safe_default_protocol_versions()
                .unwrap()
        };
        let chain = vec![cert.der().clone()];
        let key = || PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
        let server = builder()
            .with_no_client_auth()
            .with_single_cert(chain.clone(), key())
            .unwrap();
        let mut roots = RootCertStore::empty();
        roots.add(ca.der().clone()).unwrap();
        let client_verifier =
            rustls::server::WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
                .build()
                .unwrap();
        let server_wanting_client_cert = builder()
            .with_client_cert_verifier(client_verifier)
            .with_single_cert(chain, key())
            .unwrap();
        Pki {
            ca_file,
            server: Arc::new(server),
            server_wanting_client_cert: Arc::new(server_wanting_client_cert),
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
        server_with(pki.server.clone(), offer_tls).await
    }

    async fn server_with(config: Arc<ServerConfig>, offer_tls: bool) -> (u16, Events) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let events = Events::default();
        let acceptor = TlsAcceptor::from(config);
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
            // Wrong CA and wrong name: verify-ca turns a name error into success, which is
            // only right as long as rustls checks the chain first.
            fails("127.0.0.1", format!("sslmode=verify-ca&sslrootcert={wrong_ca}")),
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
    fn system_roots_need_verify_full() {
        for mode in [SslMode::Disable, SslMode::Prefer, SslMode::Require, SslMode::VerifyCa] {
            let err = Tls::new(mode, Some(RootCerts::System), None).err().unwrap();
            assert_eq!(
                err.to_string(),
                format!("weak sslmode \"{mode}\" may not be used with sslrootcert=system (use \"verify-full\")")
            );
        }
        let e = Endpoint::parse("postgres://u@localhost/db?sslrootcert=system").unwrap();
        assert_eq!(e.tls().mode, SslMode::VerifyFull);
    }

    #[tokio::test]
    async fn says_when_the_server_wants_a_client_certificate() {
        let pki = pki("client-cert");
        let (port, _) = server_with(pki.server_wanting_client_cert.clone(), true).await;
        let e = endpoint(
            "localhost",
            port,
            &format!("sslmode=verify-full&sslrootcert={}", pki.ca_file.display()),
        );
        let (raw, pg) = connect_both(&e).await;
        for err in [raw.unwrap_err(), pg.unwrap_err()] {
            assert!(format!("{err:#}").contains("wants a client certificate"), "{err:#}");
        }
    }

    #[test]
    fn verify_modes_need_a_root_certificate() {
        let err = Tls::new(SslMode::VerifyFull, None, None).err().unwrap();
        assert!(err.to_string().contains("needs a root certificate"), "{err}");
        assert!(Tls::new(SslMode::Require, None, Some("/nonexistent".into())).is_ok());
    }
}
