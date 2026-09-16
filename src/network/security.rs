use crate::core::error::DocDownloaderError;
use reqwest::dns::{Name, Resolve, Resolving};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use url::Url;

#[derive(Clone, Default)]
pub struct SecureDnsResolver;

impl Resolve for SecureDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let domain = name.as_str().to_string();
        Box::pin(async move {
            let mut addrs: Vec<SocketAddr> = Vec::new();
            if let Ok(lookup) = tokio::net::lookup_host((domain.as_str(), 0)).await {
                for addr in lookup {
                    if is_ip_allowed(addr.ip()) {
                        addrs.push(addr);
                    }
                }
            }
            if addrs.is_empty() {
                let err: Box<dyn std::error::Error + Send + Sync> = Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("No public IP addresses found for domain {}", domain),
                ));
                return Err(err);
            }
            let iter: Box<dyn Iterator<Item = SocketAddr> + Send> = Box::new(addrs.into_iter());
            Ok(iter)
        })
    }
}

/// Validates that an IP address is a public, routable internet address and not a loopback,
/// private (RFC 1918), link-local, carrier-grade NAT, or multicast address.
pub fn is_ip_allowed(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => is_ipv4_allowed(ipv4),
        IpAddr::V6(ipv6) => is_ipv6_allowed(ipv6),
    }
}

fn is_ipv4_allowed(ip: Ipv4Addr) -> bool {
    // 0.0.0.0/8 ("This network")
    if ip.octets()[0] == 0 {
        return false;
    }
    // 127.0.0.0/8 (Loopback)
    if ip.is_loopback() {
        return false;
    }
    // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16 (Private RFC 1918)
    if ip.is_private() {
        return false;
    }
    // 169.254.0.0/16 (Link-local / Cloud metadata, e.g. 169.254.169.254)
    if ip.is_link_local() {
        return false;
    }
    // 100.64.0.0/10 (Shared address space / Carrier-grade NAT)
    if ip.octets()[0] == 100 && (ip.octets()[1] & 0xC0) == 64 {
        return false;
    }
    // 192.0.0.0/24 (IETF protocol assignments)
    if ip.octets()[0] == 192 && ip.octets()[1] == 0 && ip.octets()[2] == 0 {
        return false;
    }
    // 192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24 (TEST-NET documentation)
    if (ip.octets()[0] == 192 && ip.octets()[1] == 0 && ip.octets()[2] == 2)
        || (ip.octets()[0] == 198 && ip.octets()[1] == 51 && ip.octets()[2] == 100)
        || (ip.octets()[0] == 203 && ip.octets()[1] == 0 && ip.octets()[2] == 113)
    {
        return false;
    }
    // 198.18.0.0/15 (Network benchmark tests)
    if ip.octets()[0] == 198 && (ip.octets()[1] & 0xFE) == 18 {
        return false;
    }
    // 224.0.0.0/4 (Multicast)
    if ip.is_multicast() {
        return false;
    }
    // 240.0.0.0/4 (Reserved / Future use)
    if ip.octets()[0] >= 240 {
        return false;
    }
    // 255.255.255.255 (Broadcast)
    if ip.is_broadcast() {
        return false;
    }

    true
}

fn is_ipv6_allowed(ip: Ipv6Addr) -> bool {
    // ::1 (Loopback)
    if ip.is_loopback() {
        return false;
    }
    // :: (Unspecified)
    if ip.is_unspecified() {
        return false;
    }
    // IPv4-mapped IPv6 address (::ffff:127.0.0.1, etc.)
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_ipv4_allowed(v4);
    }
    // fe80::/10 (Link-local unicast)
    let segments = ip.segments();
    if (segments[0] & 0xFFC0) == 0xFE80 {
        return false;
    }
    // fc00::/7 (Unique local address / private)
    if (segments[0] & 0xFE00) == 0xFC00 {
        return false;
    }
    // ff00::/8 (Multicast)
    if ip.is_multicast() {
        return false;
    }

    true
}

/// Validates that a target URL has a supported scheme (http or https) and does not point to
/// an SSRF target (localhost, private network, link-local, cloud metadata).
pub fn validate_url_security(url: &Url) -> Result<(), DocDownloaderError> {
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(DocDownloaderError::InvalidUrl {
            url: url.to_string(),
            reason: format!("Unsupported scheme '{scheme}': only HTTP and HTTPS are permitted"),
        });
    }

    match url.host() {
        Some(url::Host::Ipv4(ipv4)) => {
            if !is_ip_allowed(IpAddr::V4(ipv4)) {
                return Err(DocDownloaderError::RedirectRejected {
                    url: url.to_string(),
                    reason: format!(
                        "Target IP '{ipv4}' is in a private, loopback, or reserved range"
                    ),
                });
            }
        }
        Some(url::Host::Ipv6(ipv6)) => {
            if !is_ip_allowed(IpAddr::V6(ipv6)) {
                return Err(DocDownloaderError::RedirectRejected {
                    url: url.to_string(),
                    reason: format!(
                        "Target IP '{ipv6}' is in a private, loopback, or reserved range"
                    ),
                });
            }
        }
        Some(url::Host::Domain(domain)) => {
            let lower_host = domain.to_ascii_lowercase();
            if lower_host == "localhost"
                || lower_host.ends_with(".localhost")
                || lower_host.ends_with(".local")
                || lower_host.ends_with(".internal")
                || lower_host.ends_with(".lan")
                || lower_host.ends_with(".home")
                || lower_host.ends_with(".corp")
            {
                return Err(DocDownloaderError::RedirectRejected {
                    url: url.to_string(),
                    reason: format!(
                        "Target hostname '{domain}' resolved to forbidden local domain"
                    ),
                });
            }
        }
        None => {
            return Err(DocDownloaderError::InvalidUrl {
                url: url.to_string(),
                reason: "URL is missing a valid hostname".to_string(),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blocks_localhost_and_loopback() {
        assert!(!is_ip_allowed("127.0.0.1".parse().unwrap()));
        assert!(!is_ip_allowed("127.0.0.2".parse().unwrap()));
        assert!(!is_ip_allowed("::1".parse().unwrap()));
        assert!(!is_ip_allowed("::ffff:127.0.0.1".parse().unwrap()));

        let url = Url::parse("http://localhost/something").unwrap();
        assert!(validate_url_security(&url).is_err());

        let url = Url::parse("http://127.0.0.1:8080/test").unwrap();
        assert!(validate_url_security(&url).is_err());
    }

    #[test]
    fn test_blocks_private_networks_and_metadata() {
        assert!(!is_ip_allowed("10.0.0.1".parse().unwrap()));
        assert!(!is_ip_allowed("172.16.0.1".parse().unwrap()));
        assert!(!is_ip_allowed("192.168.1.1".parse().unwrap()));
        assert!(!is_ip_allowed("169.254.169.254".parse().unwrap()));

        let url = Url::parse("http://169.254.169.254/latest/meta-data/").unwrap();
        assert!(validate_url_security(&url).is_err());
    }

    #[test]
    fn test_allows_public_urls() {
        let url = Url::parse("https://www.calameo.com/read/0061133461a5012e8961a").unwrap();
        assert!(validate_url_security(&url).is_ok());

        let url = Url::parse(
            "https://ps.calameoassets.com/211022160601-3b723dd9df70eeb8937f6e31fa1d3668/p1.jpg",
        )
        .unwrap();
        assert!(validate_url_security(&url).is_ok());
    }
}
