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

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
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
    pub fn read_bytes_zero_copy(&mut self, len: usize) -> Result<&'a [u8]> {
        self.take(len)
    }
}
