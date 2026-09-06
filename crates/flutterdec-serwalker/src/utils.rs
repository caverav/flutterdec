use crate::constants::{Cid, ClassId};

pub(crate) trait SnapshotObject: std::any::Any {
    const CID: ClassId;
}

macro_rules! IMPLEMENT_SNAPSHOT_OBJECT {
    ($( $name:ty = $cid:path ),* $(,)?) => {
        $(
            impl SnapshotObject for $name {
                const CID: ClassId = $cid;
            }
        )*
    };
}

// all this mess so we can use the generic more easily, i ******* hate Rust
IMPLEMENT_SNAPSHOT_OBJECT! {
    crate::raw_object::TypeParameters = ClassId::TypeParametersCid,
    crate::raw_object::PatchClass = ClassId::PatchClassCid,
    crate::raw_object::Function = ClassId::FunctionCid,
    crate::raw_object::ClosureData = ClassId::ClosureDataCid,
    crate::raw_object::FfiTrampolineData = ClassId::FfiTrampolineDataCid,
    crate::raw_object::Field = ClassId::FieldCid,
    crate::raw_object::Script = ClassId::ScriptCid,
    crate::raw_object::Library = ClassId::LibraryCid,
    crate::raw_object::Namespace = ClassId::NamespaceCid,
    crate::raw_object::KernelProgramInfo = ClassId::KernelProgramInfoCid,
    crate::raw_object::UnlinkedCall = ClassId::UnlinkedCallCid,
    crate::raw_object::ICData = ClassId::ICDataCid,
    crate::raw_object::MegamorphicCache = ClassId::MegamorphicCacheCid,
    crate::raw_object::SubtypeTestCache = ClassId::SubtypeTestCacheCid,
    crate::raw_object::LoadingUnit = ClassId::LoadingUnitCid,
    crate::raw_object::LanguageError = ClassId::LanguageErrorCid,
    crate::raw_object::UnhandledException = ClassId::UnhandledExceptionCid,
    crate::raw_object::LibraryPrefix = ClassId::LibraryPrefixCid,
    crate::raw_object::Type = ClassId::TypeCid,
    crate::raw_object::FunctionType = ClassId::FunctionTypeCid,
    crate::raw_object::RecordType = ClassId::RecordTypeCid,
    crate::raw_object::TypeParameter = ClassId::TypeParameterCid,
    crate::raw_object::Closure = ClassId::ClosureCid,
    crate::raw_object::Double = ClassId::DoubleCid,
    crate::raw_object::Int32x4 = ClassId::Int32x4Cid,
    crate::raw_object::GrowableObjectArray = ClassId::GrowableObjectArrayCid,
    crate::raw_object::StackTrace = ClassId::StackTraceCid,
    crate::raw_object::RegExp = ClassId::RegExpCid,
    crate::raw_object::WeakProperty = ClassId::WeakPropertyCid,
    crate::raw_object::Code = ClassId::CodeCid,
    crate::raw_object::ObjectPool = ClassId::ObjectPoolCid,
    crate::raw_object::Map = ClassId::MapCid,
    crate::raw_object::Set = ClassId::SetCid,
    crate::raw_object::Class = ClassId::ClassCid,
    crate::raw_object::TypeArguments = ClassId::TypeArgumentsCid,
    crate::raw_object::ExceptionHandlers = ClassId::ExceptionHandlersCid,
    crate::raw_object::Context = ClassId::ContextCid,
    crate::raw_object::ContextScope = ClassId::ContextScopeCid,
    crate::raw_object::Mint = ClassId::MintCid,
    crate::raw_object::Float32x4 = ClassId::Float32x4Cid,
    crate::raw_object::Float64x2 = ClassId::Float64x2Cid,
    crate::raw_object::Record = ClassId::RecordCid,
    crate::raw_object::Array = ClassId::ArrayCid,
    crate::raw_object::WeakArray = ClassId::WeakArrayCid,
    crate::raw_object::ImmutableArray = ClassId::ImmutableArrayCid,
    crate::raw_object::ConstMap = ClassId::ConstMapCid,
    crate::raw_object::ConstSet = ClassId::ConstSetCid,
    crate::raw_object::CodeSourceMap = ClassId::CodeSourceMapCid,
    crate::raw_object::CompressedStackMaps = ClassId::CompressedStackMapsCid,
    crate::raw_object::PcDescriptors = ClassId::PcDescriptorsCid,
    crate::raw_object::_String = ClassId::_StringCid,
}

#[macro_export]
macro_rules! DECLARE_FIXED_LENGTH_CLUSTER {
    ($name:ident, $cluster_name:ident, |$_self:ident, $stream:ident| $fill_impl:block) => {
        DECLARE_FIXED_LENGTH_CLUSTER!($name, $cluster_name, false, |$_self, $stream| $fill_impl);
    };
    (
        $name:ident,
        $cluster_name:ident,
        $has_canonical_set_layout:literal,
        |$_self:ident, $stream:ident| $fill_impl:block
    ) => {
        #[derive(Default)]
        #[allow(clippy::vec_box)]
        pub struct $cluster_name {
            tags: u32,
            cid: Cid,
            is_immutable: bool,
            is_canonical: bool,
            pub obj_count: u64,

            start_of_fill: usize,
            start_of_alloc: usize,

            end_of_fill: usize,
            end_of_alloc: usize,

            pub first_ref_id: u32,

            pub objs: Vec<Box<$name>>,
        }

        impl Cluster for $cluster_name {
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn object_by_ref_id(&self, id: u32) -> Option<&dyn std::any::Any> {
                let index = id.checked_sub(self.first_ref_id)? as usize;
                self.objs
                    .get(index)
                    .map(|object| object.as_ref() as &dyn std::any::Any)
            }

            fn cid(&self) -> Cid {
                self.cid
            }

            fn first_ref_id(&self) -> u32 {
                self.first_ref_id
            }

            fn set_metadata(
                &mut self,
                tags: u32,
                cid: Cid,
                is_immutable: bool,
                is_canonical: bool,
            ) {
                self.tags = tags;
                self.cid = cid;
                self.is_immutable = is_immutable;
                self.is_canonical = is_canonical;
            }

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

                if $has_canonical_set_layout && self.is_canonical {
                    $crate::cluster::read_canonical_set_layout(self.obj_count, stream)?;
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
        #[allow(clippy::vec_box)]
        pub struct $cluster_name {
            tags: u32,
            cid: Cid,
            is_immutable: bool,
            is_canonical: bool,
            pub obj_count: u64,

            start_of_fill: usize,
            start_of_alloc: usize,

            end_of_fill: usize,
            end_of_alloc: usize,

            pub first_ref_id: u32,

            pub objs: Vec<Box<$name>>,
        }
    };
}

pub struct DecodedTags {
    class_id: Cid,
    is_immutable: bool,
    is_canonical: bool,
}

impl DecodedTags {
    pub fn new(cid: Cid, immut: bool, canonical: bool) -> Self {
        Self {
            class_id: cid,
            is_immutable: immut,
            is_canonical: canonical,
        }
    }

    pub fn get_cid(&self) -> Cid {
        self.class_id
    }

    pub fn is_immutable(&self) -> bool {
        self.is_immutable
    }

    pub fn is_canonical(&self) -> bool {
        self.is_canonical
    }
}

macro_rules! DECODE_CID {
    ($tags:expr) => {
        (($tags >> 12) & 0xFFFFF) as Cid
    };
}
macro_rules! DECODE_IS_IMMUTABLE {
    ($tags:expr) => {
        // Dart 3.11.1 UntaggedObject::ImmutableBit is bit 6. Mainline later
        // split this into ShallowImmutableBit (6) and DeeplyImmutableBit (7).
        (($tags >> 6) & 0x1) == 1
    };
}
macro_rules! DECODE_IS_CANONICAL {
    ($tags:expr) => {
        (($tags >> 1) & 0x1) == 1
    };
}

pub fn decode_tags(tags: u32) -> DecodedTags {
    DecodedTags::new(
        DECODE_CID!(tags),
        DECODE_IS_IMMUTABLE!(tags),
        DECODE_IS_CANONICAL!(tags),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::ClassId;

    /// UntaggedObject::TagBits, raw_object.h: ClassIdTag at bit 12 and 20 wide,
    /// CanonicalBit at 1, ImmutableBit at 6.
    fn encode_tags(cid: u32, canonical: bool, immutable: bool) -> u32 {
        (cid << 12) | ((immutable as u32) << 6) | ((canonical as u32) << 1)
    }

    #[test]
    fn decodes_the_three_header_fields_independently() {
        for (cid, canonical, immutable) in [
            (ClassId::FunctionCid, false, false),
            (ClassId::LibraryCid, true, false),
            (ClassId::_StringCid, false, true),
            (ClassId::ClassCid, true, true),
        ] {
            let d = decode_tags(encode_tags(cid as u32, canonical, immutable));
            assert_eq!(d.get_cid(), cid as u32);
            assert_eq!(d.is_canonical(), canonical, "canonical bit for {cid:?}");
            assert_eq!(d.is_immutable(), immutable, "immutable bit for {cid:?}");
        }
    }

    /// The bits must not bleed into each other. Setting only one at a time
    /// catches an off-by-one in any of the three shifts.
    #[test]
    fn the_flag_bits_do_not_overlap() {
        let only_canonical = decode_tags(encode_tags(ClassId::FunctionCid as u32, true, false));
        assert!(only_canonical.is_canonical() && !only_canonical.is_immutable());

        let only_immutable = decode_tags(encode_tags(ClassId::FunctionCid as u32, false, true));
        assert!(!only_immutable.is_canonical() && only_immutable.is_immutable());

        // Neither flag may disturb the class id.
        for (c, i) in [(false, false), (true, false), (false, true), (true, true)] {
            let d = decode_tags(encode_tags(ClassId::LibraryCid as u32, c, i));
            assert_eq!(d.get_cid(), ClassId::LibraryCid as u32);
        }
    }

    #[test]
    fn application_class_ids_remain_valid_raw_cids() {
        let application_cid = ClassId::NumPredefinedCids as u32 + 37;
        let decoded = decode_tags(encode_tags(application_cid, false, false));
        assert_eq!(decoded.get_cid(), application_cid);
    }

    /// Real headers observed on the wire: the class id occupies bits 12..32, so
    /// a two-byte cid still round trips.
    #[test]
    fn wide_class_ids_survive_the_shift() {
        let d = decode_tags(encode_tags(
            ClassId::NumPredefinedCids as u32 - 1,
            false,
            false,
        ));
        assert_eq!(d.get_cid(), ClassId::NumPredefinedCids as u32 - 1);
    }
}
