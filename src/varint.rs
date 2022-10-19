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

#[inline(always)] fn ceil_div(n: u32, d: u32) -> u32 {
    (n + d - 1) / d
}

use std::ops::Deref;

use bytes::{Buf, BufMut};

pub fn decode_varint(src: &[u8]) -> Option<(u64, usize)> {
    let msb = *src.get(0)?;
    let len = msb.leading_ones() as usize + 1;
    let rest = src.get(1..len)?;
    debug_assert!( matches!( rest.len(), 0..=8 ) );

    // apply a mask to eliminate the prefix, giving the value bits of b0
    // this is the most significant byte of the value
    let msb = msb & (0xFFu8 >> len.min(7));

    let mut val = [0u8; 8];
    let offset = 8 - rest.len(); // no overflow: rest is never more than 8 bytes
    val[offset.saturating_sub(1)] = msb; // saturating is fine: if rest takes up whole array, than it will overwrite msb
    val[offset..].copy_from_slice(rest);
    let val = u64::from_be_bytes(val);
    return Some((val, len));
}

/// read a varint from a buffer
pub fn read_varint(src: &mut impl Buf) -> u64 {
    let (val, len) = decode_varint(src.chunk()).unwrap();
    src.advance(len);
    return val;
}

/// An encoded varint
pub struct EncodedVarInt {
    buf: [u8; 9],
    /// the offset of the most significant byte in the buffer
    msb: usize
}

impl Deref for EncodedVarInt {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.buf[self.msb..]
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
        return EncodedVarInt { buf, msb: 8 }
    }

    let len = ceil_div(bitlen, 7); // result in [2, 8]
    debug_assert!( matches!( len, 2..=8 ) );

    let b0_valmask = 0xFFu8 >> len.min(7);
    let prefix_mask = !b0_valmask; // note that the zero bit is not included
    let prefix_delim = !(1u8 << (8 - len));
    debug_assert!(prefix_mask & prefix_delim != 0, "{val} {len} {b0_valmask}");
    
    let msb = (9 - len) as usize;
    buf[msb] = (buf[msb] | prefix_mask) & prefix_delim;

    return EncodedVarInt { buf, msb };
}

pub fn write_varint(dest: &mut impl BufMut, val: u64) -> usize {
    let encoded = encode_varint(val);
    dest.put_slice(&encoded);
    return encoded.len();
}

#[cfg(test)]
mod test {
    use rand::Rng;

    use super::*;

    #[test]
    pub fn read_single_byte() {
        for i in 0..127 {
            assert_eq!( read_varint(&mut &[i][..]), i as u64);
        }
    }

    #[test]
    pub fn read_knowns() {
        assert_eq!( read_varint(&mut &[0xFF;9][..]), u64::MAX);
        assert_eq!( read_varint(&mut &[
            0b1000_0001,
            0b1100_1000
        ][..]), 456);
    }

    #[test]
    pub fn write_knowns() {
        let mut buf = [0u8; 9];

        let len = write_varint(&mut &mut buf[..], 0);
        assert_eq!(&buf[..len], [0]);

        let len = write_varint(&mut &mut buf[..], u64::MAX);
        assert_eq!(&buf[..len], [0xFF; 9]);
    }

    fn test_roundtrip(val: u64) -> usize {
        let mut buf = [0u8; 9];
        let len = write_varint(&mut &mut buf[..], val);
        let decoded = read_varint(&mut &buf[..len]);
        assert_eq!(val, decoded);
        len
    }

    #[test]
    pub fn roundtrip_failures() {
        let mut buf = [0u8; 9];
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
