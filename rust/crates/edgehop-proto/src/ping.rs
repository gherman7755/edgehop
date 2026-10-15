/// Payload of a `ping` frame: the sender's monotonic milliseconds, 8 bytes big-endian. The receiver echoes it
/// in a `pong`.
pub fn ping_payload(now_millis: u64) -> [u8; 8] {
    now_millis.to_be_bytes()
}

/// Round-trip time in milliseconds from an echoed pong payload, or `None` if it is not 8 bytes.
/// Wrapping subtraction keeps a clock quirk from panicking.
pub fn rtt_millis(pong_payload: &[u8], now_millis: u64) -> Option<f64> {
    let bytes: [u8; 8] = pong_payload.try_into().ok()?;
    Some(now_millis.wrapping_sub(u64::from_be_bytes(bytes)) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_the_round_trip() {
        let payload = ping_payload(1_000);
        assert_eq!(rtt_millis(&payload, 1_012), Some(12.0));
    }

    #[test]
    fn rejects_payloads_of_the_wrong_size() {
        assert_eq!(rtt_millis(&[1, 2, 3], 5), None);
        assert_eq!(rtt_millis(&[], 5), None);
    }
}
