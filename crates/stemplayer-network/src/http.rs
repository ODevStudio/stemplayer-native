use std::io::ErrorKind;
use std::time::Duration;
use stemplayer_domain::{ErrorCode, PlayerError};

use crate::client::{HttpResponse, HttpTransport};

pub struct UreqTransport {
    agent: ureq::Agent,
}

impl Default for UreqTransport {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .max_response_header_size(32 * 1024)
            .timeout_global(Some(Duration::from_secs(15)))
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_recv_body(Some(Duration::from_secs(10)))
            .user_agent("stemplayer-native/0.1")
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

impl HttpTransport for UreqTransport {
    fn get(&self, url: &url::Url, maximum_bytes: usize) -> Result<HttpResponse, PlayerError> {
        let mut response = self
            .agent
            .get(url.as_str())
            .call()
            .map_err(|error| map_error(error, url.scheme() == "https"))?;
        if response.status().is_redirection() {
            return Err(PlayerError::new(
                ErrorCode::HlsRedirectRejected,
                "HLS redirect was rejected",
            ));
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let bytes = response
            .body_mut()
            .with_config()
            .limit(maximum_bytes as u64 + 1)
            .read_to_vec()
            .map_err(|error| map_error(error, url.scheme() == "https"))?;
        if bytes.len() > maximum_bytes {
            return Err(PlayerError::new(
                ErrorCode::HlsResponseTooLarge,
                "HLS response exceeds its byte limit",
            ));
        }
        Ok(HttpResponse {
            bytes,
            content_type,
        })
    }
}

fn map_error(error: ureq::Error, secure: bool) -> PlayerError {
    let (code, summary) = match error {
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => {
            (ErrorCode::HlsTlsFailed, "HLS TLS validation failed")
        }
        ureq::Error::TooManyRedirects | ureq::Error::RedirectFailed => {
            (ErrorCode::HlsRedirectRejected, "HLS redirect was rejected")
        }
        ureq::Error::BodyExceedsLimit(_) => (
            ErrorCode::HlsResponseTooLarge,
            "HLS response exceeds its byte limit",
        ),
        ureq::Error::StatusCode(_) => (
            ErrorCode::HlsHttpFailed,
            "HLS server returned an error status",
        ),
        ureq::Error::Timeout(_) => (ErrorCode::HlsGroupBufferingTimeout, "HLS request timed out"),
        ureq::Error::Io(error) if !secure || is_socket_failure(error.kind()) => {
            (ErrorCode::HlsHttpFailed, "HLS connection failed")
        }
        ureq::Error::Io(_) => (ErrorCode::HlsTlsFailed, "HLS TLS connection failed"),
        ureq::Error::Protocol(_) => (
            ErrorCode::HlsHttpFailed,
            "HLS server response was malformed",
        ),
        _ => (ErrorCode::HlsHttpFailed, "HLS request failed"),
    };
    PlayerError::new(code, summary).with_user_action("Check the network and retry the source")
}

fn is_socket_failure(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::ConnectionRefused
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::NotConnected
            | ErrorKind::NetworkDown
            | ErrorKind::NetworkUnreachable
            | ErrorKind::HostUnreachable
            | ErrorKind::BrokenPipe
            | ErrorKind::UnexpectedEof
            | ErrorKind::TimedOut
            | ErrorKind::Interrupted
            | ErrorKind::WouldBlock
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::process::Command;

    #[test]
    fn transport_has_no_proxy() {
        assert!(UreqTransport::default().agent.config().proxy().is_none());
    }

    #[test]
    fn ignores_ambient_proxy_credentials() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "http::tests::transport_has_no_proxy"])
            .env("ALL_PROXY", "http://synthetic:dummy@127.0.0.1:9")
            .env("NO_PROXY", "")
            .env("no_proxy", "")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    #[test]
    fn socket_failures_remain_retryable_over_https() {
        for secure in [false, true] {
            for kind in [
                ErrorKind::ConnectionRefused,
                ErrorKind::ConnectionReset,
                ErrorKind::ConnectionAborted,
                ErrorKind::NotConnected,
                ErrorKind::NetworkDown,
                ErrorKind::NetworkUnreachable,
                ErrorKind::HostUnreachable,
                ErrorKind::BrokenPipe,
                ErrorKind::UnexpectedEof,
                ErrorKind::TimedOut,
                ErrorKind::Interrupted,
                ErrorKind::WouldBlock,
            ] {
                let error = map_error(ureq::Error::Io(io::Error::from(kind)), secure);
                assert_eq!(error.code, ErrorCode::HlsHttpFailed, "{kind:?}");
            }
        }
    }

    #[test]
    fn preserves_tls_failures_including_wrapped_validation_errors() {
        let errors = [
            ureq::Error::Tls("synthetic TLS validation failure"),
            ureq::Error::Io(io::Error::new(
                ErrorKind::InvalidData,
                ureq::Error::Tls("synthetic certificate failure"),
            )),
            ureq::Error::Io(io::Error::other(io::Error::new(
                ErrorKind::InvalidData,
                ureq::Error::Tls("synthetic wrapped certificate failure"),
            ))),
        ];
        for error in errors {
            assert_eq!(map_error(error, true).code, ErrorCode::HlsTlsFailed);
        }
    }
}
