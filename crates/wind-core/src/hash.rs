//! SHA-256 (FIPS 180-4), for the fingerprint of a source file in a
//! reproducible recipe (ARQUITETURA C3): a recipe names its data by hash, so
//! a replay can tell the very file it was made from. The standard library
//! has no SHA-256, and a few lines spare a dependency; the browser side uses
//! its own `crypto.subtle`, and both are checked against the standard's
//! test vectors.

/// Bytes in a SHA-256 message block.
const BLOCK_BYTES: usize = 64;
/// Bytes of the message length appended by the padding (big-endian bits).
const LENGTH_BYTES: usize = 8;
/// The first padding byte: a single 1 bit.
const PADDING_START: u8 = 0x80;

/// The first 32 bits of the fractional parts of the cube roots of the first
/// 64 primes (FIPS 180-4, §4.2.2).
const ROUND_CONSTANTS: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
    0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
    0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
    0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
    0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// The first 32 bits of the fractional parts of the square roots of the
/// first 8 primes (FIPS 180-4, §5.3.3).
const INITIAL_STATE: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];

/// The SHA-256 digest of `bytes`.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut state = INITIAL_STATE;
    let mut blocks = bytes.chunks_exact(BLOCK_BYTES);
    for block in &mut blocks {
        compress(&mut state, block.try_into().expect("chunks_exact yields whole blocks"));
    }
    // The tail, the 1 bit, zeros, and the length in bits: one or two blocks.
    let tail = blocks.remainder();
    let mut last = [0u8; 2 * BLOCK_BYTES];
    last[..tail.len()].copy_from_slice(tail);
    last[tail.len()] = PADDING_START;
    let padded = if tail.len() + 1 + LENGTH_BYTES <= BLOCK_BYTES { BLOCK_BYTES } else { 2 * BLOCK_BYTES };
    let bits = (bytes.len() as u64).wrapping_mul(8);
    last[padded - LENGTH_BYTES..padded].copy_from_slice(&bits.to_be_bytes());
    for block in last[..padded].chunks_exact(BLOCK_BYTES) {
        compress(&mut state, block.try_into().expect("chunks_exact yields whole blocks"));
    }
    let mut digest = [0u8; 32];
    for (word, out) in state.iter().zip(digest.chunks_exact_mut(4)) {
        out.copy_from_slice(&word.to_be_bytes());
    }
    digest
}

/// The SHA-256 digest of `bytes` as 64 lowercase hex digits.
pub fn sha256_hex(bytes: &[u8]) -> String {
    sha256(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// One block of the compression function (FIPS 180-4, §6.2.2).
fn compress(state: &mut [u32; 8], block: &[u8; BLOCK_BYTES]) {
    let mut schedule = [0u32; 64];
    for (word, bytes) in schedule.iter_mut().zip(block.chunks_exact(4)) {
        *word = u32::from_be_bytes(bytes.try_into().expect("four bytes"));
    }
    for t in 16..64 {
        let (w15, w2) = (schedule[t - 15], schedule[t - 2]);
        let sigma0 = w15.rotate_right(7) ^ w15.rotate_right(18) ^ (w15 >> 3);
        let sigma1 = w2.rotate_right(17) ^ w2.rotate_right(19) ^ (w2 >> 10);
        schedule[t] = schedule[t - 16].wrapping_add(sigma0).wrapping_add(schedule[t - 7]).wrapping_add(sigma1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for (constant, word) in ROUND_CONSTANTS.iter().zip(schedule) {
        let sum1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choose = (e & f) ^ (!e & g);
        let first = h.wrapping_add(sum1).wrapping_add(choose).wrapping_add(*constant).wrapping_add(word);
        let sum0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let second = sum0.wrapping_add(majority);
        (h, g, f, e, d, c, b, a) = (g, f, e, d.wrapping_add(first), c, b, a, first.wrapping_add(second));
    }
    for (word, add) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *word = word.wrapping_add(add);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_test_vectors() {
        // FIPS 180-4 examples (NIST CSRC), and the empty message.
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            "two padding blocks"
        );
        assert_eq!(sha256_hex(&vec![b'a'; 1_000_000]), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    }
}
