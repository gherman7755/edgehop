import CryptoKit
import Foundation

// The encrypted link: Noise_XXpsk3_25519_ChaChaPoly_SHA256 (docs/PROTOCOL.md, "Encryption"), written against the
// Noise specification (revision 34) with CryptoKit's primitives and checked, byte for byte, against the Rust
// implementation (the `snow` crate): `Tests/EdgeHopCoreTests/NoiseTests.swift` and the vectors in
// `rust/crates/edgehop-proto/tests/`.
//
// The shared token is the pre-shared key, so a handshake only succeeds between computers that know it.

public enum NoiseError: Error, Equatable {
    /// A message did not verify: wrong token, damage, or an attacker.
    case crypto
    case invalid(String)
}

public enum Noise {
    public static let protocolName = "Noise_XXpsk3_25519_ChaChaPoly_SHA256"
    /// Binds the handshake to this protocol version.
    public static let prologue = Data("edgehop/2".utf8)
    public static let maxMessage = 65535
    /// Largest plaintext chunk of a transport message (the AEAD tag takes 16 bytes).
    public static let maxChunk = maxMessage - 16

    /// The 32-byte pre-shared key for a token: HMAC-SHA256(key: "edgehop-noise-psk-v1", message: token).
    public static func psk(fromToken token: String) -> Data {
        let key = SymmetricKey(data: Data("edgehop-noise-psk-v1".utf8))
        return Data(HMAC<SHA256>.authenticationCode(for: Data(token.utf8), using: key))
    }

    /// True if the first two bytes a computer sent are the "EH" magic of an older, unencrypted EdgeHop: that
    /// computer's hello frame, where a Noise message length was expected.
    public static func looksLikeOlderEdgeHop(_ firstTwo: Data) -> Bool {
        firstTwo.count >= 2 && firstTwo[firstTwo.startIndex] == 0x45 && firstTwo[firstTwo.startIndex + 1] == 0x48
    }

    /// A message with the 2-byte big-endian length in front.
    public static func withLength(_ message: Data) -> Data {
        var out = Data([UInt8(message.count >> 8), UInt8(message.count & 0xff)])
        out.append(message)
        return out
    }
}

/// Cuts a byte stream into length-prefixed messages, whatever the chunks it arrives in.
public struct NoiseMessageReader {
    private var buffer = Data()
    public init() {}

    public mutating func push(_ bytes: Data) { buffer.append(bytes) }

    /// Whatever has been pushed but is not part of a message handed out yet.
    public mutating func drain() -> Data {
        let rest = buffer
        buffer = Data()
        return rest
    }

    /// The next complete message, if there is one.
    public mutating func nextMessage() -> Data? {
        guard buffer.count >= 2 else { return nil }
        let base = buffer.startIndex
        let length = Int(buffer[base]) << 8 | Int(buffer[base + 1])
        guard buffer.count >= 2 + length else { return nil }
        let message = Data(buffer[(base + 2)..<(base + 2 + length)])
        buffer = Data(buffer[(base + 2 + length)...])
        return message
    }
}

// MARK: Noise building blocks

private struct CipherState {
    var key: SymmetricKey?
    var nonce: UInt64 = 0

    var hasKey: Bool { key != nil }

    private func nonceBytes() throws -> ChaChaPoly.Nonce {
        var data = Data(count: 4)
        var counter = nonce.littleEndian
        withUnsafeBytes(of: &counter) { data.append(contentsOf: $0) }
        return try ChaChaPoly.Nonce(data: data)
    }

    mutating func encrypt(ad: Data, plaintext: Data) throws -> Data {
        guard let key else { return plaintext }
        let sealed = try ChaChaPoly.seal(plaintext, using: key, nonce: try nonceBytes(), authenticating: ad)
        nonce += 1
        return sealed.ciphertext + sealed.tag
    }

    mutating func decrypt(ad: Data, ciphertext: Data) throws -> Data {
        guard let key else { return ciphertext }
        guard ciphertext.count >= 16 else { throw NoiseError.crypto }
        let split = ciphertext.count - 16
        let body = Data(ciphertext.prefix(split)), tag = Data(ciphertext.suffix(16))
        do {
            let box = try ChaChaPoly.SealedBox(nonce: try nonceBytes(), ciphertext: body, tag: tag)
            let plaintext = try ChaChaPoly.open(box, using: key, authenticating: ad)
            nonce += 1
            return plaintext
        } catch { throw NoiseError.crypto }
    }
}

private func hmac(_ key: Data, _ data: Data) -> Data {
    Data(HMAC<SHA256>.authenticationCode(for: data, using: SymmetricKey(data: key)))
}

/// Noise's HKDF with HMAC-SHA256: two or three 32-byte outputs.
private func hkdf(chainingKey: Data, input: Data, outputs: Int) -> [Data] {
    let temp = hmac(chainingKey, input)
    var result = [Data]()
    var previous = Data()
    for index in 1...outputs {
        previous = hmac(temp, previous + Data([UInt8(index)]))
        result.append(previous)
    }
    return result
}

private struct SymmetricState {
    var chainingKey: Data
    var hash: Data
    var cipher = CipherState()

    init(protocolName: String) {
        let name = Data(protocolName.utf8)
        hash = name.count <= 32 ? name + Data(count: 32 - name.count) : Data(SHA256.hash(data: name))
        chainingKey = hash
    }

    mutating func mixHash(_ data: Data) { hash = Data(SHA256.hash(data: hash + data)) }

    mutating func mixKey(_ input: Data) {
        let out = hkdf(chainingKey: chainingKey, input: input, outputs: 2)
        chainingKey = out[0]
        cipher = CipherState(key: SymmetricKey(data: out[1]))
    }

    mutating func mixKeyAndHash(_ input: Data) {
        let out = hkdf(chainingKey: chainingKey, input: input, outputs: 3)
        chainingKey = out[0]
        mixHash(out[1])
        cipher = CipherState(key: SymmetricKey(data: out[2]))
    }

    mutating func encryptAndHash(_ plaintext: Data) throws -> Data {
        let ciphertext = try cipher.encrypt(ad: hash, plaintext: plaintext)
        mixHash(ciphertext)
        return ciphertext
    }

    mutating func decryptAndHash(_ ciphertext: Data) throws -> Data {
        let plaintext = try cipher.decrypt(ad: hash, ciphertext: ciphertext)
        mixHash(ciphertext)
        return plaintext
    }

    func split() -> (CipherState, CipherState) {
        let out = hkdf(chainingKey: chainingKey, input: Data(), outputs: 2)
        return (CipherState(key: SymmetricKey(data: out[0])), CipherState(key: SymmetricKey(data: out[1])))
    }
}

// MARK: Handshake

/// One side of the handshake: `write` and `read` in turn, as the pattern says, until `isFinished`, then
/// `transport()`.
///
///     1. controller -> receiver: e                 (no payload)
///     2. receiver -> controller: e, ee, s, es      (no payload)
///     3. controller -> receiver: s, se, psk        (the controller's hello as payload)
public final class NoiseHandshake {
    public enum Role { case initiator, responder }

    public let role: Role
    private var symmetric = SymmetricState(protocolName: Noise.protocolName)
    private let psk: Data
    private let staticKey: Curve25519.KeyAgreement.PrivateKey
    private let fixedEphemeral: Curve25519.KeyAgreement.PrivateKey?
    private var ephemeral: Curve25519.KeyAgreement.PrivateKey?
    private var remoteStatic: Curve25519.KeyAgreement.PublicKey?
    private var remoteEphemeral: Curve25519.KeyAgreement.PublicKey?
    private var step = 0
    public private(set) var isFinished = false
    private var finished: (CipherState, CipherState)?

    public convenience init(role: Role, token: String) {
        self.init(role: role, psk: Noise.psk(fromToken: token), staticKey: nil, ephemeralKey: nil)
    }

    /// With fixed keys, for the test vectors only: never use this to talk to anyone.
    init(role: Role, psk: Data, staticKey: Data, ephemeralKey: Data) throws {
        self.role = role
        self.psk = psk
        self.staticKey = try Curve25519.KeyAgreement.PrivateKey(rawRepresentation: staticKey)
        self.fixedEphemeral = try Curve25519.KeyAgreement.PrivateKey(rawRepresentation: ephemeralKey)
        symmetric.mixHash(Noise.prologue)
    }

    private init(role: Role, psk: Data, staticKey: Curve25519.KeyAgreement.PrivateKey?, ephemeralKey: Curve25519.KeyAgreement.PrivateKey?) {
        self.role = role
        self.psk = psk
        self.staticKey = staticKey ?? Curve25519.KeyAgreement.PrivateKey()
        self.fixedEphemeral = ephemeralKey
        symmetric.mixHash(Noise.prologue)
    }

    private func newEphemeral() -> Curve25519.KeyAgreement.PrivateKey {
        fixedEphemeral ?? Curve25519.KeyAgreement.PrivateKey()
    }

    private func dh(_ priv: Curve25519.KeyAgreement.PrivateKey, _ pub: Curve25519.KeyAgreement.PublicKey) throws -> Data {
        do {
            let secret = try priv.sharedSecretFromKeyAgreement(with: pub)
            return secret.withUnsafeBytes { Data($0) }
        } catch { throw NoiseError.crypto }
    }

    private func publicKey(from data: Data) throws -> Curve25519.KeyAgreement.PublicKey {
        do { return try Curve25519.KeyAgreement.PublicKey(rawRepresentation: data) } catch { throw NoiseError.invalid("bad public key") }
    }

    /// The next message to send, carrying `payload`.
    public func write(_ payload: Data = Data()) throws -> Data {
        var out = Data()
        switch (role, step) {
        case (.initiator, 0):
            let e = newEphemeral(); ephemeral = e
            out.append(e.publicKey.rawRepresentation)
            symmetric.mixHash(e.publicKey.rawRepresentation)
            symmetric.mixKey(e.publicKey.rawRepresentation) // psk patterns also mix every ephemeral key into the key
        case (.responder, 1):
            let e = newEphemeral(); ephemeral = e
            out.append(e.publicKey.rawRepresentation)
            symmetric.mixHash(e.publicKey.rawRepresentation)
            symmetric.mixKey(e.publicKey.rawRepresentation)
            guard let re = remoteEphemeral else { throw NoiseError.invalid("out of order") }
            symmetric.mixKey(try dh(e, re)) // ee
            out.append(try symmetric.encryptAndHash(staticKey.publicKey.rawRepresentation)) // s
            symmetric.mixKey(try dh(staticKey, re)) // es (responder: s with the remote ephemeral)
        case (.initiator, 2):
            out.append(try symmetric.encryptAndHash(staticKey.publicKey.rawRepresentation)) // s
            guard let re = remoteEphemeral else { throw NoiseError.invalid("out of order") }
            symmetric.mixKey(try dh(staticKey, re)) // se (initiator: s with the remote ephemeral)
            symmetric.mixKeyAndHash(psk) // psk
        default:
            throw NoiseError.invalid("out of order")
        }
        out.append(try symmetric.encryptAndHash(payload))
        advance()
        return out
    }

    /// Processes a received message and returns its payload.
    public func read(_ message: Data) throws -> Data {
        var rest = message
        func take(_ n: Int) throws -> Data {
            guard rest.count >= n else { throw NoiseError.invalid("short message") }
            let part = Data(rest.prefix(n))
            rest = Data(rest.dropFirst(n))
            return part
        }
        switch (role, step) {
        case (.responder, 0):
            let re = try take(32)
            remoteEphemeral = try publicKey(from: re)
            symmetric.mixHash(re)
            symmetric.mixKey(re)
        case (.initiator, 1):
            let re = try take(32)
            remoteEphemeral = try publicKey(from: re)
            symmetric.mixHash(re)
            symmetric.mixKey(re)
            guard let e = ephemeral, let remote = remoteEphemeral else { throw NoiseError.invalid("out of order") }
            symmetric.mixKey(try dh(e, remote)) // ee
            let rs = try symmetric.decryptAndHash(try take(48)) // s
            remoteStatic = try publicKey(from: rs)
            symmetric.mixKey(try dh(e, remoteStatic!)) // es (initiator: e with the remote static)
        case (.responder, 2):
            let rs = try symmetric.decryptAndHash(try take(48)) // s
            remoteStatic = try publicKey(from: rs)
            guard let e = ephemeral else { throw NoiseError.invalid("out of order") }
            symmetric.mixKey(try dh(e, remoteStatic!)) // se (responder: e with the remote static)
            symmetric.mixKeyAndHash(psk) // psk
        default:
            throw NoiseError.invalid("out of order")
        }
        let payload = try symmetric.decryptAndHash(rest)
        advance()
        return payload
    }

    private func advance() {
        step += 1
        if step == 3 {
            finished = symmetric.split()
            isFinished = true
        }
    }

    /// The encrypted channel, once the handshake is finished.
    public func transport() throws -> NoiseTransport {
        guard let (first, second) = finished else { throw NoiseError.invalid("the handshake is not finished") }
        // The initiator sends with the first cipher and receives with the second; the responder the other way round.
        return role == .initiator ? NoiseTransport(send: first, receive: second) : NoiseTransport(send: second, receive: first)
    }
}

// MARK: Transport

/// The encrypted byte stream after the handshake: `seal` what is sent, `open` what arrives (in any pieces).
public struct NoiseTransport {
    private var send: CipherState
    private var receive: CipherState
    private var reader = NoiseMessageReader()

    fileprivate init(send: CipherState, receive: CipherState) {
        self.send = send
        self.receive = receive
    }

    /// Encrypts `data` as one or more length-prefixed transport messages, ready to put on the wire.
    public mutating func seal(_ data: Data) throws -> Data {
        var out = Data()
        var offset = data.startIndex
        while offset < data.endIndex {
            let end = min(offset + Noise.maxChunk, data.endIndex)
            let ciphertext = try send.encrypt(ad: Data(), plaintext: Data(data[offset..<end]))
            out.append(Noise.withLength(ciphertext))
            offset = end
        }
        return out
    }

    /// Takes bytes from the wire and returns the plaintext of every message they completed (possibly none). A throw
    /// means the connection must be dropped: a message did not verify.
    public mutating func open(_ bytes: Data) throws -> Data {
        reader.push(bytes)
        var plaintext = Data()
        while let message = reader.nextMessage() {
            plaintext.append(try receive.decrypt(ad: Data(), ciphertext: message))
        }
        return plaintext
    }
}
