//! HTTP downloads (the Download action).

/// rustls on Linux; the operating system's TLS and certificate store on
/// Windows and macOS.
pub fn agent() -> ureq::Agent {
    #[cfg(not(target_os = "linux"))]
    {
        use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
        let tls = TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build();
        return ureq::Agent::config_builder().tls_config(tls).build().new_agent();
    }
    #[allow(unreachable_code)]
    ureq::Agent::new_with_defaults()
}

pub fn get_bytes(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let mut resp = agent().get(url).call().map_err(|e| e.to_string())?;
    resp.body_mut().with_config().limit(limit).read_to_vec().map_err(|e| e.to_string())
}
