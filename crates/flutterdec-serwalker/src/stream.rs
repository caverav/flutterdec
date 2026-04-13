mod constants;

struct Stream
{
    byte_stream: &[u8],
    curr_stream_offset: usize,
}

impl Stream
{
    fn seek(&self, pos: usize)
    {
        self.curr_stream_offset = pos;
    }

    fn read_modified_uleb128(stream: &[u8]) // read a modified uleb in the current poss
    {
        let bytes = Vec<u8>
    }
}