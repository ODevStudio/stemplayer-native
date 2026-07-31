use std::time::Duration;
use stemplayer_domain::{ErrorCode, PlayerError};

use crate::client::{HttpResponse, HttpTransport};

pub struct UreqTransport {
    agent: ureq::Agent,
}

impl Default for UreqTransport {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
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
        ureq::Error::Io(_) if secure => (ErrorCode::HlsTlsFailed, "HLS TLS connection failed"),
        ureq::Error::Io(_) => (ErrorCode::HlsHttpFailed, "HLS connection failed"),
        ureq::Error::Protocol(_) => (
            ErrorCode::HlsHttpFailed,
            "HLS server response was malformed",
        ),
        _ => (ErrorCode::HlsHttpFailed, "HLS request failed"),
    };
    PlayerError::new(code, summary).with_user_action("Check the network and retry the source")
}
