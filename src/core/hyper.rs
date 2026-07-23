use std::{
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use hyper::{Request, Response, Uri, body::Incoming};
use hyper_util::client::legacy::Client as LegacyClient;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tower_service::Service;

struct NoCertVerifier;

impl std::fmt::Debug for NoCertVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoCertVerifier").finish()
    }
}

impl rustls::client::danger::ServerCertVerifier for NoCertVerifier {
    fn verify_server_cert(
        &self,
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
        ]
    }
}

pub struct EmptyBody;

impl hyper::body::Body for EmptyBody {
    type Data = &'static [u8];
    type Error = std::convert::Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
        Poll::Ready(None)
    }

    fn is_end_stream(&self) -> bool {
        true
    }
}

#[derive(Clone)]
pub struct ConnectorService {
    timeout_duration: Duration,
}

impl ConnectorService {
    pub fn new(timeout_ms: u64) -> Self {
        Self {
            timeout_duration: Duration::from_millis(timeout_ms),
        }
    }
}

impl Service<Uri> for ConnectorService {
    type Response = TokioIo<TcpStream>;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        let t_duration = self.timeout_duration;

        Box::pin(async move {
            let addr: SocketAddr = format!("{}:{}", uri.host().unwrap(), uri.port_u16().unwrap())
                .parse()
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

            let stream = timeout(t_duration, TcpStream::connect(addr))
                .await
                .map_err(|_| "connect timeout")?
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

            stream.set_nodelay(true).ok();
            #[allow(deprecated)]
            let _ = stream.set_linger(Some(std::time::Duration::ZERO));
            Ok(TokioIo::new(stream))
        })
    }
}

pub type MyHttpsConnector = hyper_rustls::HttpsConnector<ConnectorService>;
pub type MyHyperClient = LegacyClient<MyHttpsConnector, EmptyBody>;

pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub fn build_hyper_client(timeout_ms: u64) -> Option<MyHyperClient> {
    let connector = ConnectorService::new(timeout_ms);

    let tls_config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoCertVerifier))
        .with_no_client_auth();

    let https_connector = hyper_rustls::HttpsConnector::from((connector, Arc::new(tls_config)));

    let client = LegacyClient::builder(hyper_util::rt::TokioExecutor::new())
        .pool_max_idle_per_host(1)
        .pool_idle_timeout(Duration::from_secs(1))
        .build(https_connector);

    Some(client)
}

pub async fn send_request(
    client: &MyHyperClient,
    host: &str,
    uri: Uri,
    method: http::Method,
    timeout_ms: u64,
) -> Option<Response<Incoming>> {
    let req = Request::builder()
        .uri(uri)
        .method(method)
        .header("User-Agent", USER_AGENT)
        .header("Host", host)
        .body(EmptyBody)
        .ok()?;

    timeout(Duration::from_millis(timeout_ms), client.request(req))
        .await
        .ok()?
        .ok()
}

pub fn parse_url(url: &str) -> Option<(Uri, String, &'static str, String)> {
    let uri = url.parse::<Uri>().ok()?;
    let host = uri.host()?.to_string();
    let scheme = uri.scheme_str()?;
    let scheme = if scheme == "https" { "https" } else { "http" };
    let path = uri.path().to_string();
    Some((uri, host, scheme, path))
}