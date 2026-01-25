use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use std::net::IpAddr;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum UrlParseError {
    #[error("Invalid URL format: {0}")]
    InvalidUrl(String),
    #[error("URL has no host")]
    NoHost,
    #[error("URL parsing error")]
    ParseError,
}

const URI_COMPONENT_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'$')
    .add(b'&')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

const QUERY_COMPONENT_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b'"')
    .add(b'#')
    .add(b'$')
    .add(b'&')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

pub fn encode_component(s: &str) -> String {
    utf8_percent_encode(s, URI_COMPONENT_ENCODE_SET).to_string()
}

pub fn encode_query_component(s: &str) -> String {
    utf8_percent_encode(s, QUERY_COMPONENT_ENCODE_SET).to_string()
}

pub fn extract_domain(url_str: &str) -> Result<String, UrlParseError> {
    let url_to_parse = if !url_str.contains("://") {
        format!("http://{}", url_str)
    } else {
        url_str.to_string()
    };

    let parsed = Url::parse(&url_to_parse).map_err(|e| UrlParseError::ParseError)?;

    let host = parsed.host_str().ok_or_else(|| UrlParseError::NoHost)?;

    if let Ok(ip_addr) = host.parse::<IpAddr>() {
        return Ok(match ip_addr {
            IpAddr::V4(ipv4) => ipv4.to_string(),
            IpAddr::V6(ipv6) => {
                format!("[{}]", ipv6)
            }
        });
    }

    let domain = normalize_domain(host);
    Ok(domain)
}

fn normalize_domain(host: &str) -> String {
    let host_lower = host.to_lowercase();

    if host_lower.starts_with("www.") {
        host_lower[4..].to_string()
    } else {
        host_lower
    }
}

#[cfg(test)]
mod tests {
    use super::{encode_component, encode_query_component, extract_domain};

    #[test]
    fn test_encode_component() {
        assert_eq!(encode_component("hello world!"), "hello%20world!");
        assert_eq!(encode_component("Rust编程"), "Rust%E7%BC%96%E7%A8%8B");
        assert_eq!(encode_component("a=b&c+d"), "a%3Db%26c%2Bd");
    }

    #[test]
    fn test_encode_query_component() {
        assert_eq!(encode_query_component("hello world!"), "hello+world!");
        assert_eq!(encode_query_component("Rust编程"), "Rust%E7%BC%96%E7%A8%8B");
        assert_eq!(encode_query_component("a=b&c+d"), "a%3Db%26c%2Bd");
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(extract_domain("https://example.com").unwrap(), "example.com");
        assert_eq!(extract_domain("http://sub.example.com:8080").unwrap(), "sub.example.com");

        assert_eq!(extract_domain("example.com/path").unwrap(), "example.com");
        assert_eq!(extract_domain("sub.example.com").unwrap(), "sub.example.com");

        assert_eq!(extract_domain("http://192.168.1.1").unwrap(), "192.168.1.1");
        assert_eq!(extract_domain("https://[::1]:8080").unwrap(), "[::1]");

        assert_eq!(extract_domain("https://WWW.EXAMPLE.COM").unwrap(), "example.com");

        assert!(extract_domain("").is_err());
        assert!(extract_domain("://").is_err());
    }
}
