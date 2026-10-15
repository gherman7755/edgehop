use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Length of the server's random challenge.
pub const NONCE_LEN: usize = 32;
/// Length of an HMAC-SHA256 proof.
pub const PROOF_LEN: usize = 32;

const CONTEXT: &[u8] = b"edgehop-link-v1|";

fn mac(token: &str, nonce: &[u8]) -> HmacSha256 {
    // HMAC accepts keys of any length, so `new_from_slice` cannot fail.
    let mut mac =
        HmacSha256::new_from_slice(token.as_bytes()).expect("HMAC accepts any key length");
    mac.update(CONTEXT);
    mac.update(nonce);
    mac
}

/// A fresh random nonce from the operating system's generator.
pub fn make_nonce() -> Result<[u8; NONCE_LEN], getrandom::Error> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce)?;
    Ok(nonce)
}

/// `HMAC-SHA256(key = token, message = "edgehop-link-v1|" || nonce)`: the shared token itself never
/// crosses the network.
pub fn proof(token: &str, nonce: &[u8]) -> [u8; PROOF_LEN] {
    mac(token, nonce).finalize().into_bytes().into()
}

/// Constant-time check of a received proof.
pub fn verify(token: &str, nonce: &[u8], proof: &[u8]) -> bool {
    mac(token, nonce).verify_slice(proof).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_the_matching_token_and_nonce() {
        let nonce = make_nonce().unwrap();
        let good = proof("correct-horse-battery", &nonce);
        assert!(verify("correct-horse-battery", &nonce, &good));
        assert!(!verify("wrong-token-xxxxxxx", &nonce, &good));
        assert!(!verify(
            "correct-horse-battery",
            &make_nonce().unwrap(),
            &good
        ));
    }

    #[test]
    fn rejects_truncated_and_empty_proofs() {
        let nonce = [7u8; NONCE_LEN];
        let good = proof("correct-horse-battery", &nonce);
        assert!(!verify(
            "correct-horse-battery",
            &nonce,
            &good[..PROOF_LEN - 1]
        ));
        assert!(!verify("correct-horse-battery", &nonce, &[]));
    }

    #[test]
    fn nonces_are_32_bytes_and_differ() {
        let a = make_nonce().unwrap();
        assert_eq!(a.len(), NONCE_LEN);
        assert_ne!(a, make_nonce().unwrap());
    }
}
