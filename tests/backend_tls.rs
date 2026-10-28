use std::{path::Path, sync::Arc, time::Duration};

use bao_mcp::backend::{Backend, Credential};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        self, ServerConfig,
        pki_types::{CertificateDer, PrivatePkcs8KeyDer},
    },
};

// Synthetic localhost-only certificate/key fixtures; the leaf expires 2028-10-27.
// Its 2026-10-27 start date keeps validity under macOS's 825-day maximum.
// The private key is public test data and must never be used outside these tests.
const TEST_CERT: &[u8] = include_bytes!("fixtures/tls/localhost-cert.der");
const TEST_KEY: &[u8] = include_bytes!("fixtures/tls/localhost-key.der");

struct TlsServer {
    port: u16,
    task: JoinHandle<()>,
}

impl TlsServer {
    async fn start() -> Self {
        let config = ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(TEST_CERT.to_vec())],
            PrivatePkcs8KeyDer::from(TEST_KEY.to_vec()).into(),
        )
        .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            loop {
                let Ok((connection, _)) = listener.accept().await else {
                    break;
                };
                // Strict clients intentionally reject this server's certificate.
                let Ok(mut stream) = acceptor.accept(connection).await else {
                    continue;
                };
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    match stream.read(&mut buffer).await {
                        Ok(0) | Err(_) => break,
                        Ok(length) => request.extend_from_slice(&buffer[..length]),
                    }
                }
                if request.is_empty() {
                    continue;
                }
                let body = r#"{"data":{"tls":"ok"}}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len(),
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        Self { port, task }
    }

    fn backend(&self, hostname: &str, ca_file: Option<&Path>, skip_verify: bool) -> Backend {
        Backend::new(
            &format!("https://{hostname}:{}", self.port),
            "agent",
            ca_file,
            skip_verify,
            Duration::from_secs(3),
        )
        .unwrap()
    }
}

impl Drop for TlsServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn request(backend: &Backend) -> bao_mcp::Result<Value> {
    backend
        .request(
            &Credential::new("synthetic-tls-test-token").unwrap(),
            "read",
            "sys/health",
            None,
            &[],
            None,
        )
        .await
}

fn test_ca() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tls/ca.pem"
    ))
}

#[tokio::test]
async fn strict_tls_rejects_an_untrusted_certificate() {
    let server = TlsServer::start().await;
    let error = request(&server.backend("localhost", None, false))
        .await
        .unwrap_err();
    assert_eq!(error.code, "backend_unavailable");
}

#[tokio::test]
async fn custom_ca_preserves_certificate_and_hostname_verification() {
    let server = TlsServer::start().await;
    let trusted = server.backend("localhost", Some(test_ca()), false);
    assert_eq!(
        request(&trusted).await.unwrap(),
        json!({"data":{"tls":"ok"}})
    );

    // The trusted certificate has DNS:localhost only, with no IP address SAN.
    let wrong_hostname = server.backend("127.0.0.1", Some(test_ca()), false);
    let error = request(&wrong_hostname).await.unwrap_err();
    assert_eq!(error.code, "backend_unavailable");
}

#[tokio::test]
async fn explicit_skip_verify_accepts_untrusted_and_wrong_hostname_certificates() {
    let server = TlsServer::start().await;
    for hostname in ["localhost", "127.0.0.1"] {
        let backend = server.backend(hostname, None, true);
        assert_eq!(
            request(&backend).await.unwrap(),
            json!({"data":{"tls":"ok"}})
        );
    }
}
