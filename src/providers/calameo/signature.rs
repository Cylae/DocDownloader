use reqwest::header::HeaderMap;

/// Represents the public HMAC token headers returned by Calaméo's book webservice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalameoSignature {
    pub expires: u64,
    pub path: String,
    pub signature: String,
}

impl CalameoSignature {
    /// Extracts signature parameters from response HTTP headers.
    pub fn from_headers(headers: &HeaderMap) -> Option<Self> {
        let expires = headers
            .get("x-calameo-hash-expires")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())?;

        let path = headers
            .get("x-calameo-hash-path")
            .and_then(|h| h.to_str().ok())?
            .to_string();

        let signature = headers
            .get("x-calameo-hash-signature")
            .and_then(|h| h.to_str().ok())?
            .to_string();

        Some(Self {
            expires,
            path,
            signature,
        })
    }

    /// Appends the legitimate reader `_token_` query parameter required by Calaméo's CDN.
    pub fn sign_url(&self, base_url: &str) -> String {
        let token = format!(
            "exp={}~acl={}~hmac={}",
            self.expires, self.path, self.signature
        );
        let delimiter = if base_url.contains('?') { '&' } else { '?' };
        format!("{base_url}{delimiter}_token_={token}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    #[test]
    fn test_signature_extraction_and_url_signing() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-calameo-hash-expires",
            HeaderValue::from_static("1789499206"),
        );
        headers.insert(
            "x-calameo-hash-path",
            HeaderValue::from_static("%2Fkey%2F%2A"),
        );
        headers.insert(
            "x-calameo-hash-signature",
            HeaderValue::from_static("deadbeef1234"),
        );

        let sig = CalameoSignature::from_headers(&headers).expect("Failed to parse signature");
        assert_eq!(sig.expires, 1789499206);
        assert_eq!(sig.path, "%2Fkey%2F%2A");
        assert_eq!(sig.signature, "deadbeef1234");

        let signed = sig.sign_url("https://ps.calameoassets.com/key/p1.jpg");
        assert_eq!(
            signed,
            "https://ps.calameoassets.com/key/p1.jpg?_token_=exp=1789499206~acl=%2Fkey%2F%2A~hmac=deadbeef1234"
        );
    }
}
