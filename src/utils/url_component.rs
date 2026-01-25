use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

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

#[cfg(test)]
mod tests {
    use super::{encode_component, encode_query_component};

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
}
