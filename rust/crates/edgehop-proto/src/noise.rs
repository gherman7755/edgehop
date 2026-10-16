//! The encrypted link: `Noise_XXpsk3_25519_ChaChaPoly_SHA256` (`docs/PROTOCOL.md`, "Encryption").
//!
//! The shared token is the pre-shared key (hashed to 32 bytes, [`psk_from_token`]), so a handshake only succeeds
//! between computers that know it, and everything after it is encrypted and authenticated. Pure logic over byte
//! buffers: no sockets, so it is tested on any machine and its bytes are checked against the Swift implementation
//! (`tests/noise-vectors.json`).
//!
//! Handshake (every message is sent with a 2-byte big-endian length in front, as Noise recommends):
//!
//! 1. controller -> receiver: `e` (no payload)
//! 2. receiver -> controller: `e, ee, s, es` (no payload)
//! 3. controller -> receiver: `s, se, psk` and the controller's hello as payload
//!
//! The receiver learns whether the token is right from message 3 (a wrong one fails to decrypt) and answers with the
//! first encrypted frame, so the controller learns it from that frame arriving.
//!
//! After the handshake the byte stream of frames is cut into chunks of at most [`MAX_CHUNK`] bytes; each chunk is
//! one Noise transport message, again with a 2-byte length in front.

use std::sync::Arc;

use hmac::{Hmac, Mac};
use sha2::Sha256;
use snow::{params::NoiseParams, Builder, HandshakeState, StatelessTransportState};
use thiserror::Error;

/// The Noise protocol name.
pub const PATTERN: &str = "Noise_XXpsk3_25519_ChaChaPoly_SHA256";
/// Binds the handshake to this protocol version.
pub const PROLOGUE: &[u8] = b"edgehop/2";
/// Largest Noise message.
pub const MAX_MESSAGE: usize = 65535;
/// Largest plaintext chunk of a transport message (the AEAD tag takes 16 bytes).
pub const MAX_CHUNK: usize = MAX_MESSAGE - 16;

#[derive(Debug, Error)]
pub enum NoiseError {
    /// A handshake or transport message did not verify: wrong token, damage, or an attacker.
    #[error("the encrypted link failed to verify a message")]
    Crypto,
    /// Peer sent something that is not a valid Noise message here.
    #[error("invalid message in the encrypted handshake: {0}")]
    Invalid(&'static str),
}

impl From<snow::Error> for NoiseError {
    fn from(_: snow::Error) -> Self {
        Self::Crypto
    }
}

/// The 32-byte pre-shared key for a token: `HMAC-SHA256(key = "edgehop-noise-psk-v1", message = token)`.
pub fn psk_from_token(token: &str) -> [u8; 32] {
    // HMAC accepts keys of any length, so `new_from_slice` cannot fail.
    let mut mac = Hmac::<Sha256>::new_from_slice(b"edgehop-noise-psk-v1").expect("any key length");
    mac.update(token.as_bytes());
    mac.finalize().into_bytes().into()
}

/// True if the first two bytes a computer sent are the `EH` magic of an older, unencrypted EdgeHop: that
/// computer's hello frame, where a Noise message length was expected.
pub fn looks_like_older_edgehop(first_two: [u8; 2]) -> bool {
    &first_two == b"EH"
}

/// Which side of the handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The controller: speaks first.
    Initiator,
    /// The receiver.
    Responder,
}

fn params() -> NoiseParams {
    PATTERN.parse().expect("the pattern is valid")
}

/// Adds the 2-byte length in front of a Noise message.
pub fn with_length(message: &[u8]) -> Vec<u8> {
    debug_assert!(message.len() <= MAX_MESSAGE);
    let mut out = Vec::with_capacity(message.len() + 2);
    out.extend_from_slice(&(message.len() as u16).to_be_bytes());
    out.extend_from_slice(message);
    out
}

/// Cuts a byte stream into length-prefixed messages, whatever the chunks it arrives in.
#[derive(Debug, Default)]
pub struct MessageReader {
    buffer: Vec<u8>,
}

impl MessageReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// The next complete message, if there is one.
    pub fn next_message(&mut self) -> Option<Vec<u8>> {
        if self.buffer.len() < 2 {
            return None;
        }
        let length = usize::from(u16::from_be_bytes([self.buffer[0], self.buffer[1]]));
        if self.buffer.len() < 2 + length {
            return None;
        }
        let message = self.buffer[2..2 + length].to_vec();
        self.buffer.drain(..2 + length);
        Some(message)
    }

    /// Bytes received but not yet part of a complete message.
    pub fn pending(&self) -> &[u8] {
        &self.buffer
    }
}

/// One side of the handshake. Call [`Self::write`] and [`Self::read`] in turn, as the pattern says, until
/// [`Self::is_finished`], then [`Self::into_transport`].
pub struct Handshake {
    state: HandshakeState,
    role: Role,
}

impl Handshake {
    /// A fresh handshake with random keys.
    pub fn new(role: Role, token: &str) -> Result<Self, NoiseError> {
        let psk = psk_from_token(token);
        let builder = Builder::new(params());
        let keys = builder.generate_keypair()?;
        Self::build(role, &psk, &keys.private, None)
    }

    /// With fixed keys, for the test vectors only: never use this to talk to anyone.
    pub fn with_fixed_keys_for_testing(
        role: Role,
        psk: &[u8; 32],
        static_private: &[u8],
        ephemeral_private: &[u8],
    ) -> Result<Self, NoiseError> {
        Self::build(role, psk, static_private, Some(ephemeral_private))
    }

    fn build(
        role: Role,
        psk: &[u8; 32],
        static_private: &[u8],
        ephemeral: Option<&[u8]>,
    ) -> Result<Self, NoiseError> {
        let mut builder = Builder::new(params())
            .prologue(PROLOGUE)?
            .local_private_key(static_private)?
            .psk(3, psk)?;
        if let Some(ephemeral) = ephemeral {
            builder = builder.fixed_ephemeral_key_for_testing_only(ephemeral);
        }
        let state = match role {
            Role::Initiator => builder.build_initiator()?,
            Role::Responder => builder.build_responder()?,
        };
        Ok(Self { state, role })
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn is_finished(&self) -> bool {
        self.state.is_handshake_finished()
    }

    /// The next message to send, carrying `payload`.
    pub fn write(&mut self, payload: &[u8]) -> Result<Vec<u8>, NoiseError> {
        let mut out = vec![0u8; MAX_MESSAGE];
        let n = self.state.write_message(payload, &mut out)?;
        out.truncate(n);
        Ok(out)
    }

    /// Processes a received message and returns its payload.
    pub fn read(&mut self, message: &[u8]) -> Result<Vec<u8>, NoiseError> {
        let mut out = vec![0u8; MAX_MESSAGE];
        let n = self.state.read_message(message, &mut out)?;
        out.truncate(n);
        Ok(out)
    }

    /// The encrypted channel, once the handshake is finished.
    pub fn into_transport(self) -> Result<(Sealer, Opener), NoiseError> {
        if !self.is_finished() {
            return Err(NoiseError::Invalid("the handshake is not finished"));
        }
        let transport = Arc::new(self.state.into_stateless_transport_mode()?);
        Ok((
            Sealer {
                state: Arc::clone(&transport),
                nonce: 0,
                scratch: vec![0u8; MAX_MESSAGE],
            },
            Opener {
                state: transport,
                nonce: 0,
                reader: MessageReader::new(),
                scratch: vec![0u8; MAX_MESSAGE],
            },
        ))
    }
}

/// Encrypts what this side sends. Messages must reach the other side in the order they were sealed.
pub struct Sealer {
    state: Arc<StatelessTransportState>,
    nonce: u64,
    /// Output space for one message, allocated once: input events are sealed a thousand times a second.
    scratch: Vec<u8>,
}

impl Sealer {
    /// Encrypts `data` as one or more length-prefixed transport messages, ready to put on the wire.
    pub fn seal(&mut self, data: &[u8]) -> Result<Vec<u8>, NoiseError> {
        let mut out = Vec::with_capacity(data.len() + 18 * (data.len() / MAX_CHUNK + 1));
        for chunk in data.chunks(MAX_CHUNK) {
            let n = self
                .state
                .write_message(self.nonce, chunk, &mut self.scratch)?;
            self.nonce += 1;
            out.extend_from_slice(&(n as u16).to_be_bytes());
            out.extend_from_slice(&self.scratch[..n]);
        }
        Ok(out)
    }
}

/// Decrypts what the other side sends, however the bytes are cut up on the way.
pub struct Opener {
    state: Arc<StatelessTransportState>,
    nonce: u64,
    reader: MessageReader,
    scratch: Vec<u8>,
}

impl Opener {
    /// Takes bytes from the wire and returns the plaintext of every message they completed (possibly none).
    /// An error means the connection must be dropped: a message did not verify.
    pub fn open(&mut self, bytes: &[u8]) -> Result<Vec<u8>, NoiseError> {
        self.reader.push(bytes);
        let mut plaintext = Vec::new();
        while let Some(message) = self.reader.next_message() {
            let n = self
                .state
                .read_message(self.nonce, &message, &mut self.scratch)?;
            self.nonce += 1;
            plaintext.extend_from_slice(&self.scratch[..n]);
        }
        Ok(plaintext)
    }

    /// Bytes received that do not make a whole message yet (everything the handshake read ahead goes here).
    pub fn feed_leftover(&mut self, bytes: &[u8]) {
        self.reader.push(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "ABCDEFGHJKLMNPQRSTUV";

    /// Runs the whole handshake in memory and returns both channels.
    fn connect(
        controller_token: &str,
        receiver_token: &str,
        hello: &[u8],
    ) -> Result<(Sealer, Opener, Sealer, Opener, Vec<u8>), NoiseError> {
        let mut controller = Handshake::new(Role::Initiator, controller_token)?;
        let mut receiver = Handshake::new(Role::Responder, receiver_token)?;
        let m1 = controller.write(&[])?;
        assert!(receiver.read(&m1)?.is_empty());
        let m2 = receiver.write(&[])?;
        assert!(controller.read(&m2)?.is_empty());
        let m3 = controller.write(hello)?;
        let payload = receiver.read(&m3)?;
        assert!(controller.is_finished() && receiver.is_finished());
        let (cs, co) = controller.into_transport()?;
        let (rs, ro) = receiver.into_transport()?;
        Ok((cs, co, rs, ro, payload))
    }

    #[test]
    fn the_psk_depends_on_the_token_only() {
        assert_eq!(psk_from_token(TOKEN), psk_from_token(TOKEN));
        assert_ne!(
            psk_from_token(TOKEN),
            psk_from_token("ABCDEFGHJKLMNPQRSTUW")
        );
    }

    #[test]
    fn the_same_token_gives_an_encrypted_channel_in_both_directions() {
        let (mut cs, mut co, mut rs, mut ro, payload) =
            connect(TOKEN, TOKEN, b"{\"name\":\"Mac\"}").unwrap();
        assert_eq!(payload, b"{\"name\":\"Mac\"}");
        let wire = cs.seal(b"hello receiver").unwrap();
        assert!(
            !wire.windows(5).any(|w| w == b"hello"),
            "the plaintext is on the wire"
        );
        assert_eq!(ro.open(&wire).unwrap(), b"hello receiver");
        let wire = rs.seal(b"hello controller").unwrap();
        assert_eq!(co.open(&wire).unwrap(), b"hello controller");
    }

    #[test]
    fn a_wrong_token_fails_at_the_third_message() {
        let result = connect(TOKEN, "ZZZZZZZZZZZZZZZZZZZZ", b"{}");
        assert!(matches!(result, Err(NoiseError::Crypto)));
    }

    #[test]
    fn a_tampered_or_replayed_message_is_refused() {
        let (mut cs, _co, _rs, mut ro, _) = connect(TOKEN, TOKEN, b"{}").unwrap();
        let wire = cs.seal(b"input").unwrap();
        let mut damaged = wire.clone();
        *damaged.last_mut().unwrap() ^= 1;
        assert!(ro.open(&damaged).is_err());

        // a fresh pair: the same message twice (a replay) fails the second time, as the nonce moved on
        let (mut cs, _co, _rs, mut ro, _) = connect(TOKEN, TOKEN, b"{}").unwrap();
        let wire = cs.seal(b"input").unwrap();
        assert_eq!(ro.open(&wire).unwrap(), b"input");
        assert!(ro.open(&wire).is_err());
    }

    #[test]
    fn data_larger_than_one_message_is_chunked_and_reassembled_from_any_pieces() {
        let (mut cs, _co, _rs, mut ro, _) = connect(TOKEN, TOKEN, b"{}").unwrap();
        let data: Vec<u8> = (0..(MAX_CHUNK * 2 + 123))
            .map(|i| (i % 251) as u8)
            .collect();
        let wire = cs.seal(&data).unwrap();
        let mut plaintext = Vec::new();
        for piece in wire.chunks(777) {
            plaintext.extend(ro.open(piece).unwrap());
        }
        assert_eq!(plaintext, data);
    }

    #[test]
    fn empty_data_makes_no_messages() {
        let (mut cs, _co, _rs, mut ro, _) = connect(TOKEN, TOKEN, b"{}").unwrap();
        assert!(cs.seal(&[]).unwrap().is_empty());
        assert!(ro.open(&[]).unwrap().is_empty());
    }

    #[test]
    fn message_reader_reassembles_split_messages() {
        let mut reader = MessageReader::new();
        let stream = [with_length(b"one"), with_length(b"second")].concat();
        for byte in &stream[..stream.len() - 1] {
            reader.push(&[*byte]);
        }
        assert_eq!(reader.next_message().as_deref(), Some(&b"one"[..]));
        assert_eq!(reader.next_message(), None);
        reader.push(&stream[stream.len() - 1..]);
        assert_eq!(reader.next_message().as_deref(), Some(&b"second"[..]));
        assert!(reader.pending().is_empty());
    }

    #[test]
    fn an_older_unencrypted_peer_is_recognised_by_its_magic() {
        assert!(looks_like_older_edgehop(*b"EH"));
        assert!(!looks_like_older_edgehop([0x00, 0x60]));
    }
}
