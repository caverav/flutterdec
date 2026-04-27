use crate::cluster::Cluster;
use crate::constants::UNSIGNED_M;
use crate::stream::Stream;
use paste::paste;

#[macro_export]
macro_rules! DECLARE_FIXED_LENGTH_CLUSTER {
    ($name:ident, $fill_impl:block) => {
        ::paste::paste! { // this is ugly, but the language doesn't support identifier concatenation
            struct [<$name Cluster>]
            {
                tags: u32,
                obj_count: u64,

                start_of_fill: usize,
                start_of_alloc: usize,

                end_of_fill: usize,
                end_of_alloc: usize,

                objs: Vec<(u64, Box<$name>)> // a pair (ref_id, object)
            }

            impl Cluster for [<$name Cluster>]
            {
                fn read_alloc(&mut self, last_ref_id: &mut u64, stream: &mut Stream) -> usize // read tags and count
                {
                    let initial_pos = stream.get_current_pos();
                    self.start_of_alloc = initial_pos;

                    self.obj_count = stream.read_modified_leb128(UNSIGNED_M);

                    for obj_idx in 0..self.obj_count
                    {
                        self.objs.push((*last_ref_id + obj_idx, Box::<$name>::default()));
                    }

                    *last_ref_id = *last_ref_id + self.obj_count;

                    stream.get_current_pos() - initial_pos
                }

                fn read_fill(&mut self, last_ref_id: &mut u64, stream: &mut Stream) -> usize
                $fill_impl

                fn is_fixed_len(&self) -> bool
                {
                    true
                }
            }
        }
    };
}
