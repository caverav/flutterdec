use crate::constants::ClassId;

#[macro_export]
macro_rules! DECLARE_FIXED_LENGTH_CLUSTER {
    ($name:ident, $cluster_name:ident, |$_self:ident, $stream:ident| $fill_impl:block) => {
        #[derive(Default)]
        pub struct $cluster_name {
            tags: u32,
            cid: ClassId,
            obj_count: u64,

            start_of_fill: usize,
            start_of_alloc: usize,

            end_of_fill: usize,
            end_of_alloc: usize,

            first_ref_id: u32,

            objs: Vec<Box<$name>>,
        }

        impl Cluster for $cluster_name {
            fn read_alloc(
                &mut self,
                last_ref_id: &mut u64,
                stream: &mut Stream,
            ) -> anyhow::Result<usize> {
                self.start_of_alloc = stream.get_current_pos();
                self.first_ref_id = *last_ref_id as u32;

                self.obj_count = stream.read_unsigned()?;

                for _obj_idx in 0..self.obj_count {
                    self.objs.push(Box::<$name>::default());
                }

                *last_ref_id += self.obj_count;
                self.end_of_alloc = stream.get_current_pos();

                Ok(self.end_of_alloc - self.start_of_alloc)
            }

            fn read_fill(&mut self, stream: &mut Stream) -> anyhow::Result<usize> {
                self.start_of_fill = stream.get_current_pos();

                let $_self = self;
                let $stream = stream;

                $fill_impl;

                $_self.end_of_fill = $stream.get_current_pos();

                Ok($_self.end_of_fill - $_self.start_of_fill)
            }

            fn is_fixed_len(&self) -> bool {
                true
            }
        }
    };
}

#[macro_export]
macro_rules! DECLARE_VARIABLE_LENGTH_CLUSTER {
    ($name:ident, $cluster_name:ident) => {
        #[derive(Default)]
        pub struct $cluster_name {
            tags: u32,
            cid: ClassId,
            obj_count: u64,

            start_of_fill: usize,
            start_of_alloc: usize,

            end_of_fill: usize,
            end_of_alloc: usize,

            first_ref_id: u32,

            objs: Vec<Box<$name>>,
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

macro_rules! DECODE_CID {
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

/// Bit layout matches UntaggedObject::TagBits in raw_object.h:
/// ClassIdTag at 12 (20 bits), CanonicalBit at 1, ImmutableBit at 7.
pub fn decode_tags(tags: u32) -> anyhow::Result<DecodedTags> {
    let class_id = DECODE_CID!(tags).map_err(|_| {
        anyhow::anyhow!(
            "unknown class id {} in tags {tags:#x}",
            (tags >> 12) & 0xFFFFF
        )
    })?;
    Ok(DecodedTags::new(
        class_id,
        DECODE_IS_DEEPLY_IMMUTABLE!(tags),
        DECODE_IS_CANONICAL!(tags),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::ClassId;

    /// UntaggedObject::TagBits, raw_object.h: ClassIdTag at bit 12 and 20 wide,
    /// CanonicalBit at 1, ImmutableBit at 7.
    fn encode_tags(cid: u32, canonical: bool, immutable: bool) -> u32 {
        (cid << 12) | ((immutable as u32) << 7) | ((canonical as u32) << 1)
    }

    #[test]
    fn decodes_the_three_header_fields_independently() {
        for (cid, canonical, immutable) in [
            (ClassId::FunctionCid, false, false),
            (ClassId::LibraryCid, true, false),
            (ClassId::_StringCid, false, true),
            (ClassId::ClassCid, true, true),
        ] {
            let d = decode_tags(encode_tags(cid as u32, canonical, immutable)).unwrap();
            assert_eq!(d.get_cid(), cid);
            assert_eq!(d.is_canonical(), canonical, "canonical bit for {cid:?}");
            assert_eq!(
                d.is_deeply_immutable(),
                immutable,
                "immutable bit for {cid:?}"
            );
        }
    }

    /// The bits must not bleed into each other. Setting only one at a time
    /// catches an off-by-one in any of the three shifts.
    #[test]
    fn the_flag_bits_do_not_overlap() {
        let only_canonical =
            decode_tags(encode_tags(ClassId::FunctionCid as u32, true, false)).unwrap();
        assert!(only_canonical.is_canonical() && !only_canonical.is_deeply_immutable());

        let only_immutable =
            decode_tags(encode_tags(ClassId::FunctionCid as u32, false, true)).unwrap();
        assert!(!only_immutable.is_canonical() && only_immutable.is_deeply_immutable());

        // Neither flag may disturb the class id.
        for (c, i) in [(false, false), (true, false), (false, true), (true, true)] {
            let d = decode_tags(encode_tags(ClassId::LibraryCid as u32, c, i)).unwrap();
            assert_eq!(d.get_cid(), ClassId::LibraryCid);
        }
    }

    /// A cid we do not know almost always means the stream desynced upstream,
    /// so it has to surface rather than defaulting to IllegalCid.
    #[test]
    fn an_unknown_class_id_is_an_error() {
        let bogus = encode_tags(0xfffff, false, false);
        match decode_tags(bogus) {
            Ok(_) => panic!("an unknown class id must not decode"),
            Err(e) => assert!(
                e.to_string().contains("unknown class id"),
                "unexpected message: {e}"
            ),
        }
    }

    /// Real headers observed on the wire: the class id occupies bits 12..32, so
    /// a two-byte cid still round trips.
    #[test]
    fn wide_class_ids_survive_the_shift() {
        let d = decode_tags(encode_tags(
            ClassId::NumPredefinedCids as u32 - 1,
            false,
            false,
        ))
        .unwrap();
        assert_eq!(d.get_cid() as u32, ClassId::NumPredefinedCids as u32 - 1);
    }
}
