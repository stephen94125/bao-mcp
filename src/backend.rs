//! OpenBao HTTP access with request-scoped credentials.

use std::{fmt, path::Path, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::{Client, Method, StatusCode, header::HeaderValue};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use url::Url;
use zeroize::Zeroizing;

use crate::{Error, Result};

const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOKEN_BYTES: usize = 16 * 1024;

/// A caller's token, erased on drop and never included in debug output.
///
/// This deliberately has no serialization implementation or public token accessor.
#[derive(Clone)]
pub struct Credential(Zeroizing<String>);

impl Credential {
    pub fn new(token: &str) -> Result<Self> {
        if token.is_empty()
            || token.len() > MAX_TOKEN_BYTES
            || !token.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        {
            return Err(Error::unauthorized());
        }
        Ok(Self(Zeroizing::new(token.to_owned())))
    }

    /// A one-way identity used to isolate caller-specific metadata.
    pub fn hash(&self) -> String {
        hex::encode(Sha256::digest(self.0.as_bytes()))
    }

    fn token_header(&self) -> Result<HeaderValue> {
        let mut value = HeaderValue::from_str(&self.0).map_err(|_| Error::unauthorized())?;
        value.set_sensitive(true);
        Ok(value)
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credential([REDACTED])")
    }
}

/// Reusable transport and fixed namespace. It never stores a caller's token.
#[derive(Clone)]
pub struct Backend {
    client: Client,
    address: Url,
    namespace_name: String,
    namespace: Option<HeaderValue>,
    scope: String,
}

impl Backend {
    pub fn new(
        addr: &str,
        namespace: &str,
        ca_file: Option<&Path>,
        skip_verify: bool,
        timeout: Duration,
    ) -> Result<Self> {
        let address = validate_address(addr)?;
        let namespace = normalize_namespace(namespace)?;
        if timeout.is_zero() {
            return Err(Error::invalid("The OpenBao timeout must be positive"));
        }

        let mut builder = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .referer(false)
            .timeout(timeout)
            .connect_timeout(timeout);
        if skip_verify {
            builder = builder
                .tls_danger_accept_invalid_certs(true)
                .tls_danger_accept_invalid_hostnames(true);
        }
        if let Some(path) = ca_file {
            let pem = std::fs::read(path)
                .map_err(|_| Error::invalid("The OpenBao CA file could not be read"))?;
            let certificates = reqwest::Certificate::from_pem_bundle(&pem)
                .map_err(|_| Error::invalid("The OpenBao CA file must contain PEM certificates"))?;
            if certificates.is_empty() {
                return Err(Error::invalid(
                    "The OpenBao CA file contains no certificates",
                ));
            }
            for certificate in certificates {
                builder = builder.add_root_certificate(certificate);
            }
        }
        let client = builder
            .build()
            .map_err(|_| Error::invalid("The OpenBao HTTP client could not be configured"))?;

        // Length framing prevents ambiguous address/namespace concatenations.
        let mut scope = Sha256::new();
        scope.update((address.as_str().len() as u64).to_be_bytes());
        scope.update(address.as_str().as_bytes());
        scope.update(namespace.as_bytes());
        let scope = hex::encode(scope.finalize());
        let namespace_header = if namespace.is_empty() {
            None
        } else {
            Some(HeaderValue::from_str(&namespace).map_err(|_| {
                Error::invalid("The OpenBao namespace is not a valid HTTP header value")
            })?)
        };

        Ok(Self {
            client,
            address,
            namespace_name: namespace,
            namespace: namespace_header,
            scope,
        })
    }

    pub fn scope_id(&self) -> String {
        self.scope.clone()
    }

    pub fn namespace(&self) -> &str {
        &self.namespace_name
    }

    /// Make one request without redirecting or automatically retrying it.
    ///
    /// OpenBao response data is returned to this caller only. Upstream error
    /// bodies and transport details are never included in errors or logs.
    pub async fn request(
        &self,
        credential: &Credential,
        operation: &str,
        path: &str,
        body: Option<Value>,
        query: &[(String, String)],
        wrap_ttl: Option<&str>,
    ) -> Result<Value> {
        validate_path(path)?;
        let (method, mutation, selector) = match operation {
            "read" => (Method::GET, false, None),
            "write" | "create" | "update" => (Method::PUT, true, None),
            "patch" => (Method::PATCH, true, None),
            "delete" => (Method::DELETE, true, None),
            // These mappings match the pinned OpenBao API client's logical.go
            // ListWithContext and ScanWithContext methods.
            "list" => (Method::GET, false, Some("list")),
            "scan" => (Method::GET, false, Some("scan")),
            _ => return Err(Error::invalid("Unsupported OpenBao operation")),
        };
        if query.iter().any(|(key, _)| key == "list" || key == "scan") {
            return Err(Error::invalid(
                "OpenBao list and scan selectors must be supplied as operations",
            ));
        }

        let mut url = self.address.clone();
        url.set_path(&format!("/v1/{path}"));
        if !query.is_empty() || selector.is_some() {
            let mut parameters = url.query_pairs_mut();
            parameters.extend_pairs(query.iter().map(|(key, value)| (key, value)));
            if let Some(selector) = selector {
                parameters.append_pair(selector, "true");
            }
        }
        let mut request = self
            .client
            .request(method, url)
            .header("X-Vault-Token", credential.token_header()?);
        if let Some(namespace) = &self.namespace {
            request = request.header("X-Vault-Namespace", namespace.clone());
        }
        if let Some(ttl) = wrap_ttl {
            validate_wrap_ttl(ttl)?;
            request = request.header("X-Vault-Wrap-TTL", ttl);
        }
        if operation == "patch" {
            request = request.header("Content-Type", "application/merge-patch+json");
        }
        if let Some(body) = body {
            request = request.json(&body);
        }

        let mut response = request
            .send()
            .await
            .map_err(|error| transport_error(mutation, error.is_timeout()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(upstream_error(status, mutation));
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
        {
            return Err(response_too_large());
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_owned();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| transport_error(mutation, error.is_timeout()))?
        {
            if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(bytes.len()) {
                return Err(response_too_large());
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        if let Ok(value) = serde_json::from_slice(&bytes) {
            return Ok(value);
        }
        let media_type = content_type.split(';').next().unwrap_or("").trim();
        if media_type == "application/json" || media_type.ends_with("+json") {
            return Err(Error::new(
                502,
                "backend_invalid_response",
                "OpenBao returned an invalid JSON response",
            ));
        }
        if !is_binary_media_type(media_type) {
            if let Ok(text) = std::str::from_utf8(&bytes) {
                if !text.chars().any(|character| {
                    character.is_control() && !matches!(character, '\r' | '\n' | '\t')
                }) {
                    return Ok(json!({ "content_type": content_type, "body": text }));
                }
            }
        }
        Ok(json!({
            "content_type": content_type,
            "encoding": "base64",
            "body": STANDARD.encode(&bytes),
        }))
    }
}

fn validate_address(addr: &str) -> Result<Url> {
    if addr.chars().any(char::is_whitespace)
        || addr.chars().any(char::is_control)
        || addr
            .split_once("://")
            .is_some_and(|(_, rest)| rest.split('/').next().unwrap_or("").contains('@'))
    {
        return Err(Error::invalid("Invalid OpenBao address"));
    }
    let address = Url::parse(addr).map_err(|_| Error::invalid("Invalid OpenBao address"))?;
    if !matches!(address.scheme(), "http" | "https")
        || address.host_str().is_none()
        || !address.username().is_empty()
        || address.password().is_some()
        || address.query().is_some()
        || address.fragment().is_some()
        || !matches!(address.path(), "" | "/")
    {
        return Err(Error::invalid(
            "The OpenBao address must be an HTTP(S) origin without credentials, path, query or fragment",
        ));
    }
    Ok(address)
}

fn normalize_namespace(namespace: &str) -> Result<String> {
    let namespace = namespace.trim_matches('/');
    if namespace.chars().any(|character| {
        character.is_whitespace()
            || character.is_control()
            || matches!(character, '\\' | '%' | '?' | '#')
    }) || (!namespace.is_empty()
        && namespace
            .split('/')
            .any(|segment| matches!(segment, "" | "." | "..")))
    {
        return Err(Error::invalid("Invalid OpenBao namespace"));
    }
    Ok(namespace.to_owned())
}

fn validate_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path
            .chars()
            .any(|character| character.is_control() || matches!(character, '\\' | '%' | '?' | '#'))
        || Url::parse(path).is_ok()
        || path
            .trim_end_matches('/')
            .split('/')
            .any(|segment| matches!(segment, "" | "." | ".."))
        || path.ends_with("//")
    {
        return Err(Error::invalid(
            "The OpenBao path must be a relative API path without URL encoding, traversal or query syntax",
        ));
    }
    Ok(())
}

fn validate_wrap_ttl(ttl: &str) -> Result<()> {
    let split = ttl
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(ttl.len());
    let (amount, unit) = ttl.split_at(split);
    if amount.parse::<u64>().is_ok_and(|amount| amount > 0)
        && matches!(unit, "ns" | "us" | "ms" | "s" | "m" | "h")
    {
        Ok(())
    } else {
        Err(Error::invalid(
            "Response wrapping TTL must be a positive integer followed by ns, us, ms, s, m or h",
        ))
    }
}

fn transport_error(mutation: bool, timeout: bool) -> Error {
    if mutation {
        outcome_unknown(502)
    } else if timeout {
        Error::new(504, "backend_timeout", "The OpenBao request timed out")
    } else {
        Error::unavailable()
    }
}

fn outcome_unknown(status: u16) -> Error {
    Error::new(
        status,
        "outcome_unknown",
        "OpenBao did not confirm the operation; its outcome is unknown. Verify its state before trying again",
    )
}

fn upstream_error(status: StatusCode, mutation: bool) -> Error {
    match status.as_u16() {
        401 => Error::unauthorized(),
        403 => Error::forbidden(),
        404 => Error::new(404, "not_found", "The OpenBao resource was not found"),
        429 => Error::new(429, "rate_limited", "OpenBao rate limited the request"),
        500..=599 if mutation => outcome_unknown(status.as_u16()),
        500..=599 => Error::new(
            status.as_u16(),
            "backend_unavailable",
            "OpenBao is unavailable",
        ),
        300..=399 => Error::new(
            502,
            "backend_redirect",
            "OpenBao redirects are not followed",
        ),
        400..=499 => Error::new(
            status.as_u16(),
            "backend_rejected",
            "OpenBao rejected the request",
        ),
        _ => Error::new(
            502,
            "backend_error",
            "OpenBao returned an unexpected status",
        ),
    }
}

fn response_too_large() -> Error {
    Error::new(
        502,
        "backend_response_too_large",
        "The OpenBao response exceeded the 8 MiB limit",
    )
}

fn is_binary_media_type(media_type: &str) -> bool {
    media_type.starts_with("image/")
        || media_type.starts_with("audio/")
        || media_type.starts_with("video/")
        || matches!(
            media_type,
            "application/octet-stream"
                | "application/pdf"
                | "application/zip"
                | "application/pkix-cert"
                | "application/pkcs7-mime"
                | "application/pkcs12"
                | "application/x-pkcs12"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_header_is_sensitive_and_debug_is_redacted() {
        let credential = Credential::new("test-caller-token").unwrap();
        assert!(credential.token_header().unwrap().is_sensitive());
        assert!(!format!("{credential:?}").contains("test-caller-token"));
        assert_eq!(credential.hash(), credential.clone().hash());
        assert_eq!(credential.hash().len(), 64);
    }
}
