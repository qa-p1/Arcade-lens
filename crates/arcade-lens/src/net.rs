//! HTTP downloads (OCR models, the Download action).

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

/// Downloads the portable OCR models into `dir` and verifies they load.
#[cfg(feature = "ocrs")]
pub fn download_models(dir: &std::path::Path) -> Result<(), String> {
    use lens_recognizers::ocr::ocrs_engine::*;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for (url, file) in [(DETECTION_MODEL_URL, DETECTION_MODEL_FILE), (RECOGNITION_MODEL_URL, RECOGNITION_MODEL_FILE)] {
        let bytes = get_bytes(url, 200 * 1024 * 1024)?;
        let tmp = dir.join(format!("{file}.part"));
        std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, dir.join(file)).map_err(|e| e.to_string())?;
    }
    OcrsEngine::load(dir).map(|_| ()).map_err(|e| format!("downloaded models failed to load: {e}"))
}

#[cfg(not(feature = "ocrs"))]
pub fn download_models(_dir: &std::path::Path) -> Result<(), String> {
    Err("this build has no portable OCR engine".into())
}
