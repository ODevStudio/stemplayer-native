use stemplayer_domain::{ErrorCode, PlayerError};
use unicode_normalization::UnicodeNormalization;
use url::{Host, Url};

pub fn entry_url(value: &str, allow_loopback_http: bool) -> Result<Url, PlayerError> {
    let url = Url::parse(value).map_err(|_| unsafe_url("Stemset URL is invalid"))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(unsafe_url(
            "Stemset URL contains credentials, a query, or a fragment",
        ));
    }
    match url.scheme() {
        "https" => Ok(url),
        "http" if allow_loopback_http && url.host().is_some_and(loopback) => Ok(url),
        _ => Err(unsafe_url("Stemset URL must use HTTPS")),
    }
}

fn loopback(host: Host<&str>) -> bool {
    match host {
        Host::Ipv4(value) => value.is_loopback(),
        Host::Ipv6(value) => value.is_loopback(),
        Host::Domain(value) => value.eq_ignore_ascii_case("localhost"),
    }
}

pub fn child_url(base: &Url, reference: &str) -> Result<Url, PlayerError> {
    if reference.is_empty()
        || reference != reference.trim_ascii()
        || reference != reference.nfc().collect::<String>()
        || reference.starts_with('/')
        || reference.contains(['\\', '?', '#'])
        || has_scheme(reference)
    {
        return Err(unsafe_url(
            "HLS reference is not a normalized relative path",
        ));
    }
    for component in reference.split('/') {
        let decoded = percent_decode(component)?;
        if component.is_empty()
            || decoded != decoded.nfc().collect::<String>()
            || matches!(decoded.as_str(), "." | "..")
            || decoded.contains(['/', '\\'])
            || decoded.chars().any(char::is_control)
        {
            return Err(unsafe_url(
                "HLS reference contains an unsafe path component",
            ));
        }
    }
    let resolved = base
        .join(reference)
        .map_err(|_| unsafe_url("HLS reference cannot be resolved"))?;
    if resolved.origin() != base.origin() {
        return Err(unsafe_url("HLS reference changes origin"));
    }
    Ok(resolved)
}

pub fn redacted(url: &Url) -> String {
    let depth = url.path_segments().map_or(0, Iterator::count);
    format!(
        "{}://{}/{} path component(s)",
        url.scheme(),
        url.host_str().unwrap_or("invalid"),
        depth
    )
}

fn has_scheme(value: &str) -> bool {
    value.split_once(':').is_some_and(|(prefix, _)| {
        !prefix.is_empty()
            && prefix.chars().enumerate().all(|(index, ch)| {
                if index == 0 {
                    ch.is_ascii_alphabetic()
                } else {
                    ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.')
                }
            })
    })
}

fn percent_decode(value: &str) -> Result<String, PlayerError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(unsafe_url(
                    "HLS reference contains invalid percent encoding",
                ));
            }
            let high = hex(bytes[index + 1])
                .ok_or_else(|| unsafe_url("HLS reference contains invalid percent encoding"))?;
            let low = hex(bytes[index + 2])
                .ok_or_else(|| unsafe_url("HLS reference contains invalid percent encoding"))?;
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| unsafe_url("HLS reference is not UTF-8"))
}

const fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn unsafe_url(summary: &str) -> PlayerError {
    PlayerError::new(ErrorCode::HlsUrlUnsafe, summary)
        .with_user_action("Open an HTTPS stem-hls-v1 URL with safe relative references")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_entry_and_child_urls() {
        assert!(entry_url("https://example.test/stemset.json", false).is_ok());
        assert!(entry_url("http://127.0.0.1/stemset.json", true).is_ok());
        for value in [
            "http://example.test/x",
            "file:///x",
            "https://user@example.test/x",
            "https://example.test/x?token=secret",
            "https://example.test/x#f",
        ] {
            assert_eq!(
                entry_url(value, false).unwrap_err().code,
                ErrorCode::HlsUrlUnsafe
            );
        }
        let base = Url::parse("https://example.test/a/stemset.json").unwrap();
        assert_eq!(
            child_url(&base, "tracks/drums.m3u8").unwrap().as_str(),
            "https://example.test/a/tracks/drums.m3u8"
        );
        for value in [
            "/root",
            "../x",
            " /root",
            " ../x",
            "a/.. ",
            "track.m3u8 ",
            "a//b",
            "a\\b",
            "a%2fb",
            "a%5Cb",
            "%2e%2e/x",
            "a?x",
            "a#x",
            "https:x",
            "e\u{301}.m3u8",
        ] {
            assert_eq!(
                child_url(&base, value).unwrap_err().code,
                ErrorCode::HlsUrlUnsafe,
                "{value}"
            );
        }
    }
}
