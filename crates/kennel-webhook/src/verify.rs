use crate::error::{Result, WebhookError};
use axum::http::HeaderMap;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub fn verify_signature(headers: &HeaderMap, body: &[u8], secret: &str) -> Result<()> {
    // Check for Forgejo signature first
    if let Some(forgejo_sig) = headers.get("X-Forgejo-Signature") {
        let sig = forgejo_sig
            .to_str()
            .map_err(|_| WebhookError::InvalidSignature)?;
        return verify_forgejo_signature(body, secret, sig);
    }

    // Check for GitHub signature
    if let Some(github_sig) = headers.get("X-Hub-Signature-256") {
        let sig = github_sig
            .to_str()
            .map_err(|_| WebhookError::InvalidSignature)?;
        return verify_github_signature(body, secret, sig);
    }

    Err(WebhookError::MissingHeader(
        "X-Forgejo-Signature or X-Hub-Signature-256",
    ))
}

fn verify_forgejo_signature(body: &[u8], secret: &str, signature: &str) -> Result<()> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|_| WebhookError::InvalidSignature)?;
    mac.update(body);

    let expected = mac.finalize().into_bytes();
    let expected_hex = hex::encode(expected);

    if expected_hex == signature {
        Ok(())
    } else {
        Err(WebhookError::InvalidSignature)
    }
}

fn verify_github_signature(body: &[u8], secret: &str, signature: &str) -> Result<()> {
    if !signature.starts_with("sha256=") {
        return Err(WebhookError::InvalidSignature);
    }

    let signature = &signature[7..]; // Remove "sha256=" prefix

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|_| WebhookError::InvalidSignature)?;
    mac.update(body);

    let expected = mac.finalize().into_bytes();
    let expected_hex = hex::encode(expected);

    if expected_hex == signature {
        Ok(())
    } else {
        Err(WebhookError::InvalidSignature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    #[test]
    fn test_forgejo_signature_valid() {
        let body = b"test payload";
        let secret = "my-secret";

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let signature = hex::encode(mac.finalize().into_bytes());

        let mut headers = HeaderMap::new();
        headers.insert("X-Forgejo-Signature", signature.parse().unwrap());

        assert!(verify_signature(&headers, body, secret).is_ok());
    }

    #[test]
    fn test_forgejo_signature_invalid() {
        let body = b"test payload";
        let secret = "my-secret";

        let mut headers = HeaderMap::new();
        headers.insert("X-Forgejo-Signature", "invalid".parse().unwrap());

        assert!(verify_signature(&headers, body, secret).is_err());
    }

    #[test]
    fn test_github_signature_valid() {
        let body = b"test payload";
        let secret = "my-secret";

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));

        let mut headers = HeaderMap::new();
        headers.insert("X-Hub-Signature-256", signature.parse().unwrap());

        assert!(verify_signature(&headers, body, secret).is_ok());
    }

    #[test]
    fn test_github_signature_invalid() {
        let body = b"test payload";
        let secret = "my-secret";

        let mut headers = HeaderMap::new();
        headers.insert("X-Hub-Signature-256", "sha256=invalid".parse().unwrap());

        assert!(verify_signature(&headers, body, secret).is_err());
    }

    #[test]
    fn test_missing_signature() {
        let body = b"test payload";
        let secret = "my-secret";
        let headers = HeaderMap::new();

        assert!(verify_signature(&headers, body, secret).is_err());
    }
}
