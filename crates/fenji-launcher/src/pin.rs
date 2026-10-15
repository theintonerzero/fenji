//! Pins (M4 hosting spec §3.10, H1): a server's own key, named in its links. A pinned address
//! trusts exactly the certificate whose key has that pin, whatever its name and issuer, and still
//! checks the handshake's signature with it.

use std::fmt;
use std::sync::Arc;

use reqwest::Url;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, Error, SignatureScheme};
use sha2::{Digest, Sha256};

use crate::api::ApiError;

/// A server key's pin: the SHA-256 of its certificate's SubjectPublicKeyInfo, as rustls parses it
/// (Ruling HP2), written as 43 base64url characters without padding.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pin([u8; 32]);

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

impl Pin {
    /// Exactly 43 base64url characters, in their one canonical spelling; anything else is `None`.
    pub fn parse(text: &str) -> Option<Pin> {
        if text.len() != 43 {
            return None;
        }
        let mut bits = 0u32;
        let mut held = 0u32;
        let mut out = Vec::with_capacity(32);
        for c in text.bytes() {
            let v = ALPHABET.iter().position(|&a| a == c)? as u32;
            bits = (bits << 6) | v;
            held += 6;
            if held >= 8 {
                held -= 8;
                out.push((bits >> held) as u8);
                bits &= (1 << held) - 1;
            }
        }
        // The last character's two spare bits must be zero, so one pin has one spelling.
        if bits != 0 {
            return None;
        }
        Some(Pin(out.try_into().ok()?))
    }

    /// The pin of `cert`'s key.
    pub fn of_cert(cert: &CertificateDer<'_>) -> Result<Pin, Error> {
        let parsed = rustls::server::ParsedCertificate::try_from(cert)?;
        Ok(Pin(Sha256::digest(
            parsed.subject_public_key_info().as_ref(),
        )
        .into()))
    }
}

impl fmt::Display for Pin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::with_capacity(43);
        for chunk in self.0.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, &b)| n | (u32::from(b) << (16 - 8 * i)));
            for i in 0..=chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
        }
        f.write_str(&out)
    }
}

impl fmt::Debug for Pin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Pin({self})")
    }
}

/// A server's address as players give it: its origin (`https://host[:port]`, or `http://` on this
/// machine), and the pin of its own key when its link carried one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAddr {
    pub origin: String,
    pub pin: Option<Pin>,
}

impl ServerAddr {
    /// `https://host[:port]`, the same with `/#pin=<pin>`, or `http://` on this machine without a
    /// pin. A path, a query, a login or any other fragment is [`ApiError::BadUrl`]; plain http off
    /// this machine is [`ApiError::Insecure`].
    pub fn parse(text: &str) -> Result<ServerAddr, ApiError> {
        let text = text.trim();
        let (address, fragment) = match text.split_once('#') {
            Some((address, fragment)) => (address, Some(fragment)),
            None => (text, None),
        };
        let url = Url::parse(address).map_err(|_| ApiError::BadUrl)?;
        if url.host_str().is_none_or(str::is_empty)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || !matches!(url.path(), "" | "/")
        {
            return Err(ApiError::BadUrl);
        }
        let pin = match fragment {
            None => None,
            Some(f) => Some(
                f.strip_prefix("pin=")
                    .and_then(Pin::parse)
                    .ok_or(ApiError::BadUrl)?,
            ),
        };
        match url.scheme() {
            "https" => {}
            "http" if crate::api::loopback(&url) => {
                if pin.is_some() {
                    return Err(ApiError::BadUrl);
                }
            }
            "http" => return Err(ApiError::Insecure),
            _ => return Err(ApiError::BadUrl),
        }
        Ok(ServerAddr {
            origin: url.origin().ascii_serialization(),
            pin,
        })
    }
}

/// Trusts exactly the certificate whose key has the pin, whatever its name, issuer or dates, and
/// still checks each handshake signature with that key, through rustls's own checks.
#[derive(Debug)]
pub struct PinnedVerifier {
    pin: Pin,
    provider: Arc<CryptoProvider>,
}

impl PinnedVerifier {
    pub fn new(pin: Pin) -> Self {
        Self {
            pin,
            provider: Arc::new(rustls::crypto::ring::default_provider()),
        }
    }
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        if Pin::of_cert(end_entity)? == self.pin {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(Error::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

    use super::*;
    use crate::api::{ApiClient, ApiError};

    const PIN: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    #[test]
    fn a_pin_is_43_base64url_characters_and_reads_back() {
        let pin = Pin::parse("6QFQfpzx9v37ULM885PDlbUAQse-jH9cH7nfRVOL9Lo").unwrap();
        assert_eq!(
            pin.to_string(),
            "6QFQfpzx9v37ULM885PDlbUAQse-jH9cH7nfRVOL9Lo"
        );
        assert_eq!(Pin::parse(PIN).unwrap().to_string(), PIN);
        for bad in [
            "",
            "short",
            &"A".repeat(42),
            &"A".repeat(44),
            &format!("{}=", "A".repeat(42)),
            &format!("{}+", "A".repeat(42)),
            &format!("{}/", "A".repeat(42)),
        ] {
            assert!(Pin::parse(bad).is_none(), "{bad:?}");
        }
    }

    /// The server's own known answer (fenji-server's tls.rs), so the two sides can't drift.
    #[test]
    fn the_pin_of_a_certificate_is_the_one_openssl_computes() {
        let pem = include_bytes!("../../fenji-server/tests/fixtures/pin-known-answer.pem");
        let cert = CertificateDer::from_pem_slice(pem).unwrap();
        assert_eq!(
            Pin::of_cert(&cert).unwrap().to_string(),
            "6QFQfpzx9v37ULM885PDlbUAQse-jH9cH7nfRVOL9Lo"
        );
    }

    #[test]
    fn addresses_take_a_pin_only_over_https() {
        let a = ServerAddr::parse("https://203.0.113.5:7801").unwrap();
        assert_eq!(
            (a.origin.as_str(), a.pin),
            ("https://203.0.113.5:7801", None)
        );
        let a = ServerAddr::parse(&format!("https://203.0.113.5:7801/#pin={PIN}")).unwrap();
        assert_eq!(
            (a.origin.as_str(), a.pin.unwrap().to_string()),
            ("https://203.0.113.5:7801", PIN.to_string())
        );
        let a = ServerAddr::parse(&format!("https://play.example.net#pin={PIN}")).unwrap();
        assert_eq!(a.origin, "https://play.example.net");
        assert_eq!(
            ServerAddr::parse("http://127.9.9.9:7801/").unwrap().origin,
            "http://127.9.9.9:7801"
        );
        for local in [
            "http://127.0.0.1:7801",
            "http://localhost:7801",
            "http://[::1]:7801",
        ] {
            assert!(ServerAddr::parse(local).is_ok(), "{local}");
        }
        assert!(matches!(
            ServerAddr::parse("http://play.example.net"),
            Err(ApiError::Insecure)
        ));
        assert!(matches!(
            ServerAddr::parse("http://10.0.0.1:7801"),
            Err(ApiError::Insecure)
        ));
        assert!(matches!(
            ServerAddr::parse(&format!("http://127.0.0.1:7801/#pin={PIN}")),
            Err(ApiError::BadUrl)
        ));
        for bad in [
            "ftp://play.example.net",
            "play.example.net",
            "https://",
            "https://play.example.net/api",
            "https://play.example.net/#pin=short",
            "https://play.example.net/#invite=ABCD-EFGH-JKMN-PQRS",
        ] {
            assert!(
                matches!(ServerAddr::parse(bad), Err(ApiError::BadUrl)),
                "{bad}"
            );
        }
    }

    const INFO: &str = r#"{"name":"P","version":"0.5.0","pinned_build":"10015727","gate_host":"127.0.0.1","gate_port":7800,"server_id":1,"api_version":1}"#;

    /// A TLS server presenting `cert`, proving it with `key`, and answering every request with
    /// its facts.
    async fn tls_server(cert: CertificateDer<'static>, key: PrivateKeyDer<'static>) -> u16 {
        crate::test_support::tls_server(cert, key, |_| (std::time::Duration::ZERO, INFO)).await
    }

    fn pkcs8(key: &rcgen::KeyPair) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()))
    }

    /// The signature checks can't become a stub: a server that shows the pinned certificate but
    /// signs the handshake with another key is refused, while the same server with the right key
    /// is read.
    #[tokio::test]
    async fn the_pinned_certificate_without_its_key_is_refused() {
        let a = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert = a.cert.der().clone();
        let pin = Pin::of_cert(&cert).unwrap();
        let honest = tls_server(cert.clone(), pkcs8(&a.signing_key)).await;
        let addr = ServerAddr::parse(&format!("https://127.0.0.1:{honest}/#pin={pin}")).unwrap();
        assert_eq!(
            ApiClient::new(&addr)
                .unwrap()
                .server_info()
                .await
                .unwrap()
                .name,
            "P"
        );
        let other = rcgen::KeyPair::generate().unwrap();
        let impostor = tls_server(cert, pkcs8(&other)).await;
        let addr = ServerAddr::parse(&format!("https://127.0.0.1:{impostor}/#pin={pin}")).unwrap();
        let e = ApiClient::new(&addr)
            .unwrap()
            .server_info()
            .await
            .unwrap_err();
        assert!(matches!(e, ApiError::Http(_)), "{e:?}");
    }
}
