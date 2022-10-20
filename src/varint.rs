//! Format overview:
//!
//! Prefix-encoded variable-length integers.
//!
//! The first byte contains:
//!
//! 0ddd_dddd [0, 128)
//! 1ddd_dddd [0, 128)
//!
//! The number of leading ones in the first byte is the number of additional bytes
//!
//! 0 -> 1 byte           7        = 7  = 7*1
//! 10 -> 2 bytes         6 + 8    = 14 = 7*2
//! 110 -> 3 bytes        5 + 8*2  = 21 = 7*3
//! 1110 -> 4 bytes       4 + 8*3  = 28 = 7*4
//! 1111_0 -> 5 bytes     3 + 8*4  = 35 = 7*5
//! 1111_10 -> 6 bytes    2 + 8*5  = 42 = 7*6
//! 1111_110 -> 7 bytes   1 + 8*6  = 49 = 7*7
//! 1111_1110 -> 8 bytes  0 + 8*7  = 56 = 8*7
//! 1111_1111 -> 9 bytes  0 + 8*8  = 64
//!
//!
//! for example, take the number 456
//!
//! 456 = 0b1__1100_1000 (9 bits long)
//!
//! we need two bytes for encoding. formula: max(ceil(bit_len / 7), 9)
//!
//! our mask will look like
//! 10??_???? ????_????
//!
//! add in the value
//! 1000_0001 1100_1000
//!
//! this is our output!
//!
//! to decode:
//!
//! look at our first byte, there is 1 leading zero, so varint is 2 bytes long.
//!
//! mask out the prefix of the first byte, this gives us the msb of the value
//! 0000_0001
//!
//! now, concatenate the masked first byte along with the remaining bytes
//!
//! 0000_0001 1100_1000
//!
//! left pad with 0 bytes to get a big endian 64 bit number
//!
//! 0000_0000 0000_0000 0000_0000 0000_0000 0000_0000 0000_0000 0000_0001 1100_1000
//!
//! If the first byte is 0xFF, then the value bits of that byte can be ignored (masks to 0).
//! simply read the next 8 bytes as a normal 64 bit integer.

use bytes::Buf;
use std::ops::Deref;

#[inline(always)]
fn ceil_div(n: u32, d: u32) -> u32 {
    (n + d - 1) / d
}

/// Returns the length of a varint, given its most significant bit
#[inline(always)]
pub fn decode_varint_len(msb: u8) -> usize {
    return msb.leading_ones() as usize + 1;
}

/// Decode a varint of known length
///
/// Length of src must be correct (call [`decode_varint_len`] first!), or you may get incorrect results or panic
pub fn decode_varint_unchecked(src: &[u8]) -> u64 {
    let len = src.len();
    // mask for the most significant bits
    let msb_mask = match len {
        1..=7 => 0xFFu8 >> len,
        8 => 0,
        // special case for length 9, just read as a normal uint64
        9 => return u64::from_be_bytes(src[1..].try_into().unwrap()),
        // the length must have been already checked, so only [1, 9] is possible
        _ => unreachable!("decode_varint_unchecked called with invalid length"),
    };

    let mut buf = [0; 8];
    let offset = 8 - len;
    buf[offset..].copy_from_slice(src);
    buf[offset] &= msb_mask;
    return u64::from_be_bytes(buf);
}

/// Decode a varint, returns None if src does not have enough characters.
pub fn decode_varint(src: &[u8]) -> Option<u64> {
    let len = decode_varint_len(*src.get(0)?);
    Some(decode_varint_unchecked(src.get(0..len)?))
}

/// Read a varint from a buffer
pub fn read_varint(src: &mut impl Buf) -> u64 {
    let buf = src.chunk();
    let len = decode_varint_len(buf[0]);
    let val = decode_varint_unchecked(&buf[..len]);
    src.advance(len);
    return val;
}

/// An owned, encoded varint
pub struct EncodedVarInt {
    buf: [u8; 9],
    /// the offset of the most significant byte in the buffer
    msb: u8,
}

impl Deref for EncodedVarInt {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.buf[self.msb as usize..]
    }
}

/// Encode a varint
pub fn encode_varint(val: u64) -> EncodedVarInt {
    let mut buf = [0; 9];
    buf[1..].copy_from_slice(&val.to_be_bytes());
    let bitlen = u64::BITS - val.leading_zeros();
    if bitlen > 56 {
        buf[0] = 0xFF;
        return EncodedVarInt { buf, msb: 0 };
    }
    if bitlen < 8 {
        return EncodedVarInt { buf, msb: 8 };
    }

    let len = ceil_div(bitlen, 7); // result in [2, 8]
    debug_assert!(matches!(len, 2..=8));

    let b0_valmask = 0xFFu8 >> len.min(7);
    let prefix_mask = !b0_valmask; // note that the zero bit is not included
    let prefix_delim = !(1u8 << (8 - len));
    debug_assert!(prefix_mask & prefix_delim != 0, "{val} {len} {b0_valmask}");

    let msb = (9 - len) as usize;
    buf[msb] = (buf[msb] | prefix_mask) & prefix_delim;

    return EncodedVarInt {
        buf,
        msb: msb as u8,
    };
}

#[cfg(test)]
mod test {
    use rand::Rng;

    use super::*;

    #[test]
    pub fn read_single_byte() {
        for i in 0..127 {
            assert_eq!(read_varint(&mut &[i][..]), i as u64);
        }
    }

    #[test]
    pub fn read_knowns() {
        assert_eq!(read_varint(&mut &[0xFF; 9][..]), u64::MAX);
        assert_eq!(read_varint(&mut &[0b1000_0001, 0b1100_1000][..]), 456);
    }

    #[test]
    pub fn encode_knowns() {
        assert_eq!(&encode_varint(0)[..], [0]);
        assert_eq!(&encode_varint(u64::MAX)[..], [0xFF; 9]);
    }

    fn test_roundtrip(val: u64) -> usize {
        let encoded = encode_varint(val);
        let decoded = read_varint(&mut &encoded[..]);
        assert_eq!(val, decoded);
        encoded.len()
    }

    #[test]
    pub fn roundtrip_failures() {
        for val in [0, 33108953738072179] {
            test_roundtrip(val);
        }
    }

    #[test]
    pub fn roundtrips() {
        let mut rng = rand::thread_rng();

        for _ in 0..100_000 {
            let val: u64 = rng.gen();
            test_roundtrip(val);
        }
    }
}
