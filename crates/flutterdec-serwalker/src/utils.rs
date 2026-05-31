use paste::paste;

use crate::constants::ClassId;

#[macro_export]
macro_rules! DECLARE_FIXED_LENGTH_CLUSTER {
    ($name:ident, |$_self:ident, $stream:ident| $fill_impl:block) => {
        ::paste::paste! {
            pub struct [<$name Cluster>]
            {
                tags: u32,
                cid: ClassId,
                obj_count: u64,

                start_of_fill: usize,
                start_of_alloc: usize,

                end_of_fill: usize,
                end_of_alloc: usize,

                first_ref_id: u32,

                objs: Vec<Box<$name >>
            }

            impl Cluster for [<$name Cluster>]
            {
                fn read_alloc(&mut self, last_ref_id: &mut u64, stream: &mut Stream) -> usize
                {
                    self.start_of_alloc = stream.get_current_pos();
                    self.first_ref_id = *last_ref_id as u32;

                    self.obj_count = stream.read_modified_leb128(UNSIGNED_M);

                    for _obj_idx in 0..self.obj_count
                    {
                        self.objs.push(Box::<$name >::default());
                    }

                    *last_ref_id = *last_ref_id + self.obj_count;
                    self.end_of_alloc = stream.get_current_pos();

                    self.end_of_alloc - self.start_of_alloc
                }

                fn read_fill(&mut self, stream: &mut Stream) -> usize
                {
                    self.start_of_fill = stream.get_current_pos();

                    let $_self = self;
                    let $stream = stream;

                    $fill_impl;

                    $_self.end_of_fill = $stream.get_current_pos();

                    $_self.end_of_fill - $_self.start_of_fill
                }

                fn is_fixed_len(&self) -> bool
                {
                    true
                }
            }

        }
    };
}

#[macro_export]
macro_rules! DECLARE_VARIABLE_LENGTH_CLUSTER {
    ($name:ident) => {
        ::paste::paste! {
            pub struct [<$name Cluster>]
            {
                tags: u32,
                cid: ClassId,
                obj_count: u64,

                start_of_fill: usize,
                start_of_alloc: usize,

                end_of_fill: usize,
                end_of_alloc: usize,

                first_ref_id: u32,

                objs: Vec<Box<$name >>
            }
        }
    };
}

pub struct DecodedTags {
    class_id: ClassId,
    is_deeply_immutable: bool,
    is_canonical: bool,
}

impl DecodedTags {
    pub fn new(cid: ClassId, immut: bool, canonical: bool) -> Self {
        Self {
            class_id: cid,
            is_deeply_immutable: immut,
            is_canonical: canonical,
        }
    }

    pub fn get_cid(&self) -> ClassId {
        self.class_id
    }

    pub fn is_deeply_immutable(&self) -> bool {
        self.is_deeply_immutable
    }

    pub fn is_canonical(&self) -> bool {
        self.is_canonical
    }
}

macro_rules! DECODE_IS_CID {
    ($tags:expr) => {
        ClassId::try_from(($tags >> 12) & 0xFFFFF)
    };
}
macro_rules! DECODE_IS_DEEPLY_IMMUTABLE {
    ($tags:expr) => {
        (($tags >> 7) & 0x1) == 1
    };
}
macro_rules! DECODE_IS_CANONICAL {
    ($tags:expr) => {
        (($tags >> 1) & 0x1) == 1
    };
}

pub fn decode_tags(tags: u32) -> DecodedTags {
    let class_id: ClassId = DECODE_IS_CID!(tags).unwrap();
    let is_deeply_immutable: bool = DECODE_IS_DEEPLY_IMMUTABLE!(tags);
    let is_canonical: bool = DECODE_IS_CANONICAL!(tags);

    DecodedTags::new(class_id, is_deeply_immutable, is_canonical)
}
