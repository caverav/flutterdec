use anyhow::{anyhow, bail, Result};

use crate::constants::{DATA_BITS_PER_BYTE, SIGNED_M, UNSIGNED_M, UNSIGNED_MAX_DATA_PER_BYTE};

pub struct Stream<'a> {
    byte_stream: &'a [u8],
    curr_stream_offset: usize,
}

impl<'a> Stream<'a> {
    pub fn new(byte_stream: &'a [u8]) -> Self {
        Self {
            byte_stream,
            curr_stream_offset: 0,
        }
    }

    pub fn seek(&mut self, pos: usize) -> Result<()> {
        if pos > self.byte_stream.len() {
            bail!(
                "seek to {pos} past end of snapshot ({})",
                self.byte_stream.len()
            );
        }
        self.curr_stream_offset = pos;
        Ok(())
    }

    pub fn align_stream(&mut self, alignment: usize) -> anyhow::Result<()> {
        let mut next_pos = self.get_current_pos();
        if next_pos % alignment == 0 {
            return Ok(());
        }

        next_pos = next_pos & !(alignment - 1);
        next_pos += alignment;

        self.seek(next_pos)
    }

    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self
            .curr_stream_offset
            .checked_add(n)
            .filter(|e| *e <= self.byte_stream.len())
            .ok_or_else(|| {
                anyhow!(
                    "read of {n} bytes past end of snapshot at offset {}",
                    self.curr_stream_offset
                )
            })?;
        let slice = &self.byte_stream[self.curr_stream_offset..end];
        self.curr_stream_offset = end;
        Ok(slice)
    }

    pub fn get_current_pos(&self) -> usize {
        self.curr_stream_offset
    }

    /// Dart's modified LEB128: little endian 7 bit groups, continuation bit 0,
    /// final byte has its MSb set. ReadStream::Read<T>, datastream.h:231
    fn read_leb128(&mut self, end_byte_marker: u8) -> Result<u64> {
        let mut value: u64 = 0;
        let mut shift: usize = 0;
        loop {
            let byte = self.read_byte()?;
            if byte > UNSIGNED_MAX_DATA_PER_BYTE {
                // Final byte. wrapping_sub mimics C++ unsigned underflow, so the
                // signed variant sign extends and narrowing casts stay congruent.
                let tail = (byte as u64).wrapping_sub(end_byte_marker as u64);
                return Ok(value | (tail << shift));
            }
            value |= (byte as u64) << shift;
            shift += DATA_BITS_PER_BYTE;
            if shift >= 64 {
                bail!(
                    "LEB128 value exceeds 64 bits at offset {}",
                    self.curr_stream_offset
                );
            }
        }
    }

    /// ReadStream::Read<T>(), datastream.h:153. kEndByteMarker == 0xC0.
    pub fn read(&mut self) -> Result<u64> {
        self.read_leb128(SIGNED_M)
    }

    /// ReadStream::ReadUnsigned<T>(), datastream.h:99. kEndUnsignedByteMarker == 0x80.
    pub fn read_unsigned(&mut self) -> Result<u64> {
        self.read_leb128(UNSIGNED_M)
    }

    /// Raw fixed width little endian, header fields only. Mostly everything else inside the
    /// clustered body is LEB128. Renamed so the two can't get mixed up.
    pub fn read_raw_u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("take(8) is 8 bytes"),
        ))
    }

    pub fn read_raw_u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("take(4) is 4 bytes"),
        ))
    }

    pub fn read_byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn read_c_string(&mut self) -> Result<String> {
        let nul = self.byte_stream[self.curr_stream_offset..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| {
                anyhow!(
                    "unterminated C string at offset {}",
                    self.curr_stream_offset
                )
            })?;
        let raw = self.take(nul + 1)?;
        Ok(String::from_utf8(raw[..nul].to_vec())?)
    }

    /// Dart OneByteString payload is Latin-1, not UTF-8. See the string cluster comment.
    pub fn read_latin1(&mut self, len: usize) -> Result<String> {
        Ok(self.take(len)?.iter().map(|&b| b as char).collect())
    }

    /// ReadStream::ReadRefId(), datastream.h:103.
    /// Big endian VLI, not LEB128. Dart caps it at 5 stages (28 bits).
    pub fn read_ref_id(&mut self) -> Result<u32> {
        let mut result: i64 = 0;
        for _ in 0..5 {
            let byte = self.read_byte()? as i8;
            result = (result << 7) + byte as i64;
            if byte < 0 {
                return Ok((result + 128) as u32);
            }
        }
        bail!(
            "ref id longer than 5 bytes at offset {}",
            self.curr_stream_offset
        )
    }

    /// Reads a block of bytes and returns a newly allocated Vec<u8> (Creates a copy)
    pub fn read_bytes(&mut self, len: usize) -> Result<Vec<u8>> {
        Ok(self.take(len)?.to_vec())
    }

    /// Reads a block of bytes and returns a reference to the slice (Zero-copy, highly recommended for large payloads)
    pub fn read_bytes_zero_copy(&mut self, len: usize) -> Result<&[u8]> {
        self.take(len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dart's WriteStream, mirrored: 7-bit little endian groups, every byte but
    /// the last has its MSb clear, the last carries `marker + remaining_bits`.
    /// datastream.h:231
    fn encode_leb128(mut value: u64, marker: u8) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let low = (value & 0x7f) as u8;
            let rest = value >> 7;
            // The last group is the one that still fits under the marker.
            if rest == 0 && low <= (0xff - marker) {
                out.push(low + marker);
                return out;
            }
            out.push(low);
            value = rest;
        }
    }

    /// Dart's ReadRefId is big endian, seven bits per byte, terminator has the
    /// MSb set, and the reader adds 128 back. datastream.h:103
    fn encode_ref_id(value: u32) -> Vec<u8> {
        let mut groups = Vec::new();
        let mut v = value;
        loop {
            groups.push((v & 0x7f) as u8);
            v >>= 7;
            if v == 0 {
                break;
            }
        }
        groups.reverse();
        let last = groups.len() - 1;
        groups[last] |= 0x80;
        groups
    }

    #[test]
    fn read_unsigned_round_trips() {
        for v in [
            0u64,
            1,
            63,
            64,
            127,
            128,
            255,
            0x7000,
            0xffff,
            u32::MAX as u64,
        ] {
            let bytes = encode_leb128(v, UNSIGNED_M);
            assert_eq!(
                Stream::new(&bytes).read_unsigned().unwrap(),
                v,
                "unsigned round trip failed for {v:#x} encoded as {bytes:02x?}"
            );
        }
    }

    #[test]
    fn read_signed_round_trips() {
        for v in [0u64, 1, 63, 0x7000, 0xffff] {
            let bytes = encode_leb128(v, SIGNED_M);
            assert_eq!(
                Stream::new(&bytes).read().unwrap(),
                v,
                "signed round trip failed for {v:#x} encoded as {bytes:02x?}"
            );
        }
    }

    /// The distinction that produced real bugs: `Read<T>` uses kEndByteMarker
    /// (0xC0) and `ReadUnsigned<T>` uses kEndUnsignedByteMarker (0x80). Decoding
    /// with the wrong one is silently off by 64 in the final group, so this test
    /// fails the moment the two are swapped.
    #[test]
    fn the_two_markers_are_not_interchangeable() {
        // cid 7 (Function) shifted into ClassIdTag position.
        let tags = 0x7000u64;
        let signed = encode_leb128(tags, SIGNED_M);
        let unsigned = encode_leb128(tags, UNSIGNED_M);
        assert_ne!(signed, unsigned, "the two encodings must differ");

        assert_eq!(Stream::new(&signed).read().unwrap(), tags);
        assert_eq!(Stream::new(&unsigned).read_unsigned().unwrap(), tags);

        // Cross-decoding is wrong, and wrong by the marker delta in the top group.
        assert_ne!(Stream::new(&signed).read_unsigned().unwrap(), tags);
        assert_ne!(Stream::new(&unsigned).read().unwrap(), tags);
    }

    /// Negative values come back as the two's complement bit pattern, so a
    /// narrowing cast at the call site recovers the original.
    #[test]
    fn signed_reads_sign_extend() {
        assert_eq!(Stream::new(&[0xbf]).read().unwrap() as i64, -1);
        assert_eq!(Stream::new(&[0xc0]).read().unwrap() as i64, 0);
        assert_eq!(Stream::new(&[0x80]).read().unwrap() as i64, -64);
        assert_eq!(Stream::new(&[0x80]).read().unwrap() as i8, -64);
    }

    #[test]
    fn read_ref_id_round_trips() {
        for v in [0u32, 1, 127, 128, 129, 1000, 41337, (1 << 28) - 1] {
            let bytes = encode_ref_id(v);
            assert_eq!(
                Stream::new(&bytes).read_ref_id().unwrap(),
                v,
                "ref id round trip failed for {v} encoded as {bytes:02x?}"
            );
        }
    }

    /// Ref ids are big endian while everything else is little endian. If someone
    /// "unifies" them onto the LEB128 decoder this catches it: the two encodings
    /// of 128 differ, and each decoder rejects the other's bytes.
    #[test]
    fn ref_ids_are_big_endian_not_leb128() {
        let as_ref = encode_ref_id(128);
        let as_leb = encode_leb128(128, UNSIGNED_M);
        assert_eq!(as_ref, vec![0x01, 0x80]);
        assert_eq!(as_leb, vec![0x00, 0x81]);
        assert_eq!(Stream::new(&as_ref).read_ref_id().unwrap(), 128);
        assert_ne!(Stream::new(&as_leb).read_ref_id().unwrap(), 128);
    }

    /// Dart unrolls exactly five stages and asserts past that ("256MB is enough
    /// for anyone"). The input below terminates properly, so only the stage cap
    /// can reject it: eight continuation bytes then a terminator.
    #[test]
    fn ref_id_is_bounded_to_five_stages() {
        let mut overlong = vec![0x01u8; 8];
        overlong.push(0x80);
        assert!(Stream::new(&overlong).read_ref_id().is_err());
        // Five stages is still accepted.
        let ok = [0x01u8, 0x01, 0x01, 0x01, 0x80];
        assert!(Stream::new(&ok).read_ref_id().is_ok());
        // And truncation is caught too.
        assert!(Stream::new(&[0x01; 3]).read_ref_id().is_err());
    }

    /// `Deserializer::Read<T>` with sizeof(T) == 1 is ReadStream::ReadByte, a
    /// plain `*current_++`, not LEB128. A byte under 0x80 is a complete value
    /// for read_byte and a continuation byte for the LEB decoder, which is
    /// exactly how the byte-sized cluster fields desynced.
    #[test]
    fn read_byte_is_raw_not_leb128() {
        assert_eq!(Stream::new(&[0x03, 0x07]).read_byte().unwrap(), 3);

        let mut raw = Stream::new(&[0x03, 0x07]);
        assert_eq!(raw.read_byte().unwrap(), 3);
        assert_eq!(
            raw.get_current_pos(),
            1,
            "read_byte consumes exactly one byte"
        );

        let mut leb = Stream::new(&[0x03, 0x07]);
        let _ = leb.read();
        assert!(
            leb.get_current_pos() > 1,
            "the LEB decoder keeps going past 0x03"
        );
    }

    #[test]
    fn raw_fixed_width_reads_are_little_endian() {
        assert_eq!(
            Stream::new(&[0xf5, 0xf5, 0xdc, 0xdc])
                .read_raw_u32()
                .unwrap(),
            crate::constants::MAGIC_BYTES
        );
        assert_eq!(
            Stream::new(&[1, 0, 0, 0, 0, 0, 0, 0])
                .read_raw_u64()
                .unwrap(),
            1
        );
    }

    /// OneByteString is Latin-1: every byte maps to the code point of the same
    /// value. from_utf8 would reject 0xE9 on its own.
    #[test]
    fn latin1_accepts_the_high_half() {
        assert_eq!(Stream::new(&[0xe9]).read_latin1(1).unwrap(), "\u{e9}");
        assert_eq!(
            Stream::new(&[0x41, 0xff]).read_latin1(2).unwrap(),
            "A\u{ff}"
        );
        let all: Vec<u8> = (0u8..=255).collect();
        let decoded = Stream::new(&all).read_latin1(256).unwrap();
        assert_eq!(decoded.chars().count(), 256);
        assert_eq!(decoded.chars().last(), Some('\u{ff}'));
    }

    #[test]
    fn c_strings_stop_at_the_nul_and_consume_it() {
        let mut s = Stream::new(b"abc\0rest");
        assert_eq!(s.read_c_string().unwrap(), "abc");
        assert_eq!(s.get_current_pos(), 4);
    }

    /// Every read is bounds checked. We parse blobs pulled out of third party
    /// APKs, so a truncated snapshot has to be an error, never a panic.
    #[test]
    fn truncated_input_errors_instead_of_panicking() {
        assert!(Stream::new(&[]).read_byte().is_err());
        assert!(
            Stream::new(&[0x00, 0x00]).read_unsigned().is_err(),
            "no terminator"
        );
        assert!(Stream::new(&[0x41]).read_c_string().is_err(), "no nul");
        assert!(Stream::new(&[0x00, 0x00]).read_raw_u32().is_err());
        assert!(Stream::new(&[1, 2, 3]).read_latin1(4).is_err());
        assert!(Stream::new(&[1, 2, 3]).read_bytes(4).is_err());
        assert!(
            Stream::new(&[0x01; 12]).read_unsigned().is_err(),
            "overlong LEB"
        );
    }

    #[test]
    fn seek_is_bounds_checked_and_position_tracks_reads() {
        let mut s = Stream::new(&[0u8; 8]);
        assert!(s.seek(8).is_ok(), "seeking to the end is valid");
        assert!(s.seek(9).is_err());
        s.seek(0).unwrap();
        s.read_raw_u32().unwrap();
        assert_eq!(s.get_current_pos(), 4);
    }

    #[test]
    fn zero_copy_and_copying_byte_reads_agree() {
        let data = [1u8, 2, 3, 4];
        assert_eq!(Stream::new(&data).read_bytes(3).unwrap(), vec![1, 2, 3]);
        assert_eq!(
            Stream::new(&data).read_bytes_zero_copy(3).unwrap(),
            &data[..3]
        );
    }
}
