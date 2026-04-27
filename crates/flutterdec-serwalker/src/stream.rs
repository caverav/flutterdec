use crate::constants::{
    DATA_BITS_PER_BYTE, SIGNED_END_OF_DATA_BYTE, UNSIGNED_END_OF_DATA_BYTE,
    UNSIGNED_MAX_DATA_PER_BYTE,
};
pub struct Stream<'a> {
    byte_stream: &'a [u8],
    curr_stream_offset: usize,
}

impl<'a> Stream<'a> {
    fn seek(&mut self, pos: usize) // might be useful?
    {
        if self.byte_stream.len() > pos && pos >= 0 {
            self.curr_stream_offset = pos;
        }
    }

    pub fn advance_pos(&mut self, num_bytes: usize) {
        self.curr_stream_offset += num_bytes;
    }

    pub fn get_current_pos(&self) -> usize {
        self.curr_stream_offset
    }
    /*
       Reads a modified uleb from the current stream offset.

       Dart uses a modified LEB128 format. The normal format uses bytes with their MSb set in order
       to signify that there are more bytes ahead, and the last byte has its MSb unset, whereas Dart's
       implementation does the opposite. The "continuation" bit on each byte is 0, and the last byte
       has its MSb set.
    */
    pub fn read_modified_leb128(&mut self, sign_marker: u8) -> u64 // 8 bytes should be enough for anything...
    {
        let mut idx: u8 = 0;

        let first_byte = self.byte_stream[self.curr_stream_offset];
        if first_byte > UNSIGNED_MAX_DATA_PER_BYTE
        // if the first byte has its MSb set
        {
            self.advance_pos(1);
            // wrapping_sub mimics C++ unsigned underflow, giving us perfect sign-extension
            // for negative numbers, while behaving normally for positive numbers. gotta get used to this :)
            return (first_byte as u64).wrapping_sub(sign_marker as u64);
        }

        let mut read_num: u64 = 0;
        let mut byte: u8;

        loop {
            byte = self.byte_stream[self.curr_stream_offset + idx as usize];
            if byte & UNSIGNED_END_OF_DATA_BYTE == UNSIGNED_END_OF_DATA_BYTE {
                break;
            } // final byte
            read_num |= (byte as u64) << (idx as usize * DATA_BITS_PER_BYTE);
            idx += 1;
        }

        self.advance_pos((idx + 1) as usize); // advance the stream position

        // Same wrapping trick for the final byte
        let final_chunk = (byte as u64).wrapping_sub(sign_marker as u64);
        read_num |= final_chunk << (idx as usize * DATA_BITS_PER_BYTE);

        read_num
    }

    pub fn read_u64(&mut self) -> u64 {
        let u64_size = std::mem::size_of::<u64>();
        let num_slice =
            &self.byte_stream[self.curr_stream_offset..self.curr_stream_offset + u64_size];

        let converted_slice: [u8; 8] = num_slice.try_into().expect("Slice wasn't 8 bytes long...");

        self.advance_pos(u64_size);

        u64::from_le_bytes(converted_slice)
    }

    pub fn read_u32(&mut self) -> u32 {
        let u32_size = std::mem::size_of::<u32>();
        let num_slice =
            &self.byte_stream[self.curr_stream_offset..self.curr_stream_offset + u32_size];

        let converted_slice: [u8; 4] = num_slice.try_into().expect("Slice wasn't 4 bytes long...");

        self.advance_pos(u32_size);

        u32::from_le_bytes(converted_slice)
    }

    /*
       Panics if it isn't possible to create a stream from the utf-8 representation stored in
       the byte slice. It shouldn't happen, so the best possible outcome is to assume some
       logic mistake has been made and end the application. It should be a good thing to change this to an
       unwrap_or_else so that we can also print the stream offset and cluster where this error occurred, in order
       to have static debug info.
    */
    pub fn read_c_string(&mut self) -> String {
        
        let first_nullbyte_pos = 
             self.byte_stream[self.curr_stream_offset..]
            .iter()
            .position(|&b| b == 0x00)
            .expect("Reading a string until the end of the stream? Something definitely went wrong...");

        let raw_str = &self.byte_stream[self.curr_stream_offset..self.curr_stream_offset + first_nullbyte_pos];
        self.advance_pos(raw_str.len() + 1);

        String::from_utf8(raw_str.to_vec()).unwrap() // it should be horrible if for some reason a string just isn't there
    }
}
