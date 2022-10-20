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

#[inline(always)]
fn ceil_div(n: u32, d: u32) -> u32 {
    (n + d - 1) / d
}

/// Returns the length of a varint, given its most significant bit
#[inline(always)]
pub fn decode_varint_len(msb: u8) -> usize {
    return msb.leading_ones() as usize + 1;
}

/// Decode a varint of known length. You should probably use [`read_varint`] or [`decode_varint`] instead.
/// 
/// `src.len()` be correctly set (use [`decode_varint_len`]) or this function may return incorrect results or panic.
/// However, undefined behavior is never possible.
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

/// Read a varint from a [`bytes::Buf`], advancing the buffer
#[cfg(feature = "bytes")]
pub fn read_varint(src: &mut impl bytes::Buf) -> u64 {
    let buf = src.chunk();
    let len = decode_varint_len(buf[0]);
    let val = decode_varint_unchecked(&buf[..len]);
    src.advance(len);
    return val;
}

/// Encode a varint, returns size of the varint
pub fn encode_varint(val: u64, buf: &mut [u8]) -> usize {
    let bitlen = u64::BITS - val.leading_zeros();
    let len = ceil_div(bitlen, 7);
    match len {
        0..=1 => {
            buf[0] = val as u8;
            1
        }
        2..=8 => {
            let len_prefix = (0xFFu16 << (9 - len)) as u8; // cast from u16 because overflowing right-shift is undefined
            let msb_mask = (0xFFu16 >> len) as u8;
            let len = len as usize;
            buf[..len].copy_from_slice(&val.to_be_bytes()[8 - len..]);
            buf[0] = (buf[0] & msb_mask) | len_prefix;
            len
        },
        9.. => {
            buf[0] = 0xFF;
            buf[1..].copy_from_slice(&val.to_be_bytes());
            9
        },
    }
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
        let mut buf = [0; 9];
        let len = encode_varint(0, &mut buf);
        assert_eq!(&buf[..len], [0]);
        let len = encode_varint(u64::MAX, &mut buf);
        assert_eq!(&buf[..len], [0xFF; 9]);
    }

    fn test_roundtrip(val: u64) -> usize {
        let mut buf = [0; 9];
        let len = encode_varint(val, &mut buf);
        let decoded = read_varint(&mut &buf[..len]);
        assert_eq!(val, decoded);
        len
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
