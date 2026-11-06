//! URLs for the Stremio add-on protocol.

use crate::error::{Error, Result};
use url::Url;

/// Normalize what a person pasted into a manifest URL. Accepts `stremio://`
/// deep links and URLs with or without `/manifest.json`. Plain HTTP is allowed
/// only for add-ons on this machine, since a URL can carry credentials.
pub fn manifest_url(input: &str) -> Result<Url> {
    let raw = input.trim();
    let raw = match raw.strip_prefix("stremio://") {
        Some(rest) => format!("https://{rest}"),
        None => raw.to_owned(),
    };
    let mut url = Url::parse(&raw)
        .map_err(|_| Error::InvalidAddon("That doesn’t look like an add-on address."))?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback)) {
        return Err(Error::InvalidAddon(
            "Add-on addresses must start with https://.",
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::InvalidAddon(
            "Add-on addresses can’t include a username or password.",
        ));
    }
    if url.host_str().is_none() {
        return Err(Error::InvalidAddon(
            "That doesn’t look like an add-on address.",
        ));
    }
    url.set_fragment(None);
    if !url.path().ends_with("/manifest.json") {
        url.path_segments_mut()
            .map_err(|_| Error::InvalidAddon("That doesn’t look like an add-on address."))?
            .pop_if_empty()
            .push("manifest.json");
    }
    Ok(url)
}

/// Each resource argument occupies one URL path segment. Configured add-on
/// paths and query parameters must survive catalog and metadata requests.
pub fn resource_url(
    manifest: &str,
    resource: &str,
    kind: &str,
    id: &str,
    extras: &[(&str, &str)],
) -> Result<Url> {
    let mut url = Url::parse(manifest).map_err(|_| Error::Unsupported)?;
    if !matches!(url.scheme(), "http" | "https") || !url.path().ends_with("/manifest.json") {
        return Err(Error::Unsupported);
    }
    url.set_fragment(None);
    {
        let mut segments = url.path_segments_mut().map_err(|_| Error::Unsupported)?;
        segments.pop().push(resource).push(kind);
        if extras.is_empty() {
            segments.push(&format!("{id}.json"));
        } else {
            segments.push(id);
        }
    }
    if !extras.is_empty() {
        // Extras have their own escaping because '&' and '=' delimit pairs in
        // Stremio's extra path segment. set_path avoids encoding '%' twice.
        fn escape(s: &str) -> String {
            url::form_urlencoded::byte_serialize(s.as_bytes())
                .collect::<String>()
                .replace('+', "%20")
        }
        let encoded = extras
            .iter()
            .map(|(key, value)| format!("{}={}", escape(key), escape(value)))
            .collect::<Vec<_>>()
            .join("&");
        let path = format!("{}/{encoded}.json", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CINEMETA;

    #[test]
    fn resource_preserves_addon_configuration_and_escapes_segments() {
        let url = resource_url(
            "https://example.org/token/config/manifest.json?key=secret",
            "catalog",
            "movie",
            "top",
            &[("search", "a/b & café")],
        )
        .unwrap();
        assert_eq!(
            url.path(),
            "/token/config/catalog/movie/top/search=a%2Fb%20%26%20caf%C3%A9.json"
        );
        assert_eq!(url.query(), Some("key=secret"));
        let url = resource_url(CINEMETA, "meta", "movie", "bad/id?query", &[]).unwrap();
        assert!(url.path().ends_with("bad%2Fid%3Fquery.json"));
    }

    #[test]
    fn manifest_links_normalize_and_unsafe_ones_are_refused() {
        let ok = |input: &str| manifest_url(input).unwrap().to_string();
        assert_eq!(
            ok(" stremio://a.example/x/manifest.json "),
            "https://a.example/x/manifest.json"
        );
        assert_eq!(
            ok("https://a.example/cfg"),
            "https://a.example/cfg/manifest.json"
        );
        assert_eq!(
            ok("https://a.example/cfg/"),
            "https://a.example/cfg/manifest.json"
        );
        assert_eq!(
            ok("https://a.example/m/manifest.json?k=1#x"),
            "https://a.example/m/manifest.json?k=1"
        );
        assert_eq!(
            ok("http://127.0.0.1:7000/"),
            "http://127.0.0.1:7000/manifest.json"
        );
        for bad in [
            "http://a.example/",
            "ftp://a.example/",
            "nonsense",
            "https://u:p@a.example/",
        ] {
            assert!(manifest_url(bad).is_err(), "{bad} should be refused");
        }
    }
}
