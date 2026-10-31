use crate::constants::{Cid, ClassKind};
use crate::profile_reader::profile::ClassTagKind;
use crate::profile_reader::profile_json_dictionary;

pub(crate) trait SnapshotObject: std::any::Any {
    const KIND: ClassKind;
    const CID_NAME: &'static str;
}

macro_rules! IMPLEMENT_SNAPSHOT_OBJECT {
    ($( $name:ty = ($cid:path, $cid_name:path) ),* $(,)?) => {
        $(
            impl SnapshotObject for $name {
                const KIND: ClassKind = $cid;
                const CID_NAME: &'static str = $cid_name;
            }
        )*
    };
}

// all this mess so we can use the generic more easily, i ******* hate Rust
IMPLEMENT_SNAPSHOT_OBJECT! {
    crate::raw_object::TypeParameters = (ClassKind::TypeParameters, profile_json_dictionary::TYPE_PARAMETERS),
    crate::raw_object::PatchClass = (ClassKind::PatchClass, profile_json_dictionary::PATCH_CLASS),
    crate::raw_object::Function = (ClassKind::Function, profile_json_dictionary::FUNCTION),
    crate::raw_object::ClosureData = (ClassKind::ClosureData, profile_json_dictionary::CLOSURE_DATA),
    crate::raw_object::FfiTrampolineData = (ClassKind::FfiTrampolineData, profile_json_dictionary::FFI_TRAMPOLINE_DATA),
    crate::raw_object::Field = (ClassKind::Field, profile_json_dictionary::FIELD),
    crate::raw_object::Script = (ClassKind::Script, profile_json_dictionary::SCRIPT),
    crate::raw_object::Library = (ClassKind::Library, profile_json_dictionary::LIBRARY),
    crate::raw_object::Namespace = (ClassKind::Namespace, profile_json_dictionary::NAMESPACE),
    crate::raw_object::KernelProgramInfo = (ClassKind::KernelProgramInfo, profile_json_dictionary::KERNEL_PROGRAM_INFO),
    crate::raw_object::UnlinkedCall = (ClassKind::UnlinkedCall, profile_json_dictionary::UNLINKED_CALL),
    crate::raw_object::ICData = (ClassKind::ICData, profile_json_dictionary::IC_DATA),
    crate::raw_object::MegamorphicCache = (ClassKind::MegamorphicCache, profile_json_dictionary::MEGAMORPHIC_CACHE),
    crate::raw_object::SubtypeTestCache = (ClassKind::SubtypeTestCache, profile_json_dictionary::SUBTYPE_TEST_CACHE),
    crate::raw_object::LoadingUnit = (ClassKind::LoadingUnit, profile_json_dictionary::LOADING_UNIT),
    crate::raw_object::LanguageError = (ClassKind::LanguageError, profile_json_dictionary::LANGUAGE_ERROR),
    crate::raw_object::UnhandledException = (ClassKind::UnhandledException, profile_json_dictionary::UNHANDLED_EXCEPTION),
    crate::raw_object::LibraryPrefix = (ClassKind::LibraryPrefix, profile_json_dictionary::LIBRARY_PREFIX),
    crate::raw_object::Type = (ClassKind::Type, profile_json_dictionary::TYPE),
    crate::raw_object::FunctionType = (ClassKind::FunctionType, profile_json_dictionary::FUNCTION_TYPE),
    crate::raw_object::RecordType = (ClassKind::RecordType, profile_json_dictionary::RECORD_TYPE),
    crate::raw_object::TypeParameter = (ClassKind::TypeParameter, profile_json_dictionary::TYPE_PARAMETER),
    crate::raw_object::Closure = (ClassKind::Closure, profile_json_dictionary::CLOSURE),
    crate::raw_object::Double = (ClassKind::Double, profile_json_dictionary::DOUBLE),
    crate::raw_object::Int32x4 = (ClassKind::Int32x4, profile_json_dictionary::INT32X4),
    crate::raw_object::GrowableObjectArray = (ClassKind::GrowableObjectArray, profile_json_dictionary::GROWABLE_OBJECT_ARRAY),
    crate::raw_object::StackTrace = (ClassKind::StackTrace, profile_json_dictionary::STACK_TRACE),
    crate::raw_object::RegExp = (ClassKind::RegExp, profile_json_dictionary::REG_EXP),
    crate::raw_object::WeakProperty = (ClassKind::WeakProperty, profile_json_dictionary::WEAK_PROPERTY),
    crate::raw_object::Code = (ClassKind::Code, profile_json_dictionary::CODE),
    crate::raw_object::ObjectPool = (ClassKind::ObjectPool, profile_json_dictionary::OBJECT_POOL),
    crate::raw_object::Map = (ClassKind::Map, profile_json_dictionary::MAP),
    crate::raw_object::Set = (ClassKind::Set, profile_json_dictionary::SET),
    crate::raw_object::Class = (ClassKind::Class, profile_json_dictionary::CLASS),
    crate::raw_object::TypeArguments = (ClassKind::TypeArguments, profile_json_dictionary::TYPE_ARGUMENTS),
    crate::raw_object::ExceptionHandlers = (ClassKind::ExceptionHandlers, profile_json_dictionary::EXCEPTION_HANDLERS),
    crate::raw_object::Context = (ClassKind::Context, profile_json_dictionary::CONTEXT),
    crate::raw_object::ContextScope = (ClassKind::ContextScope, profile_json_dictionary::CONTEXT_SCOPE),
    crate::raw_object::Mint = (ClassKind::Mint, profile_json_dictionary::MINT),
    crate::raw_object::Float32x4 = (ClassKind::Float32x4, profile_json_dictionary::FLOAT32X4),
    crate::raw_object::Float64x2 = (ClassKind::Float64x2, profile_json_dictionary::FLOAT64X2),
    crate::raw_object::Record = (ClassKind::Record, profile_json_dictionary::RECORD),
    crate::raw_object::Array = (ClassKind::Array, profile_json_dictionary::ARRAY),
    crate::raw_object::WeakArray = (ClassKind::WeakArray, profile_json_dictionary::WEAK_ARRAY),
    crate::raw_object::ImmutableArray = (ClassKind::ImmutableArray, profile_json_dictionary::IMMUTABLE_ARRAY),
    crate::raw_object::ConstMap = (ClassKind::ConstMap, profile_json_dictionary::CONST_MAP),
    crate::raw_object::ConstSet = (ClassKind::ConstSet, profile_json_dictionary::CONST_SET),
    crate::raw_object::CodeSourceMap = (ClassKind::CodeSourceMap, profile_json_dictionary::CODE_SOURCE_MAP),
    crate::raw_object::CompressedStackMaps = (ClassKind::CompressedStackMaps, profile_json_dictionary::COMPRESSED_STACK_MAPS),
    crate::raw_object::PcDescriptors = (ClassKind::PcDescriptors, profile_json_dictionary::PC_DESCRIPTORS),
    crate::raw_object::_String = (ClassKind::_String, profile_json_dictionary::STRING),
}

#[macro_export]
macro_rules! DECLARE_FIXED_LENGTH_CLUSTER {
    // macro overload for cases where there's a canonical set layout embedded in the cluster stream
    ($name:ident, $cluster_name:ident) => {
        $crate::DECLARE_VARIABLE_LENGTH_CLUSTER!($name, $cluster_name);
        $crate::IMPLEMENT_PROFILED_CLUSTER!($name, $cluster_name, true);
    };
}

#[macro_export]
macro_rules! DECLARE_VARIABLE_LENGTH_CLUSTER {
    ($name:ident, $cluster_name:ident) => {
        #[derive(Default)]
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
    decode_tags_for_kind(tags, ClassTagKind::ObjectHeader)
}

pub fn decode_tags_for_kind(tags: u32, kind: ClassTagKind) -> DecodedTags {
    match kind {
        ClassTagKind::PackedCidAndCanonical => DecodedTags::new(tags >> 1, false, tags & 1 != 0),
        ClassTagKind::ObjectHeader => DecodedTags::new(
            DECODE_CID!(tags),
            DECODE_IS_IMMUTABLE!(tags),
            DECODE_IS_CANONICAL!(tags),
        ),
        ClassTagKind::ObjectHeaderDeeplyImmutable => DecodedTags::new(
            DECODE_CID!(tags),
            tags & (1 << 7) != 0,
            DECODE_IS_CANONICAL!(tags),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::ClassKind;

    /// UntaggedObject::TagBits, raw_object.h: ClassIdTag at bit 12 and 20 wide,
    /// CanonicalBit at 1, ImmutableBit at 6.
    fn encode_tags(cid: u32, canonical: bool, immutable: bool) -> u32 {
        (cid << 12) | ((immutable as u32) << 6) | ((canonical as u32) << 1)
    }

    #[test]
    fn decodes_the_three_header_fields_independently() {
        for (cid, canonical, immutable) in [
            (ClassKind::Function, false, false),
            (ClassKind::Library, true, false),
            (ClassKind::_String, false, true),
            (ClassKind::Class, true, true),
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
        let only_canonical = decode_tags(encode_tags(ClassKind::Function as u32, true, false));
        assert!(only_canonical.is_canonical() && !only_canonical.is_immutable());

        let only_immutable = decode_tags(encode_tags(ClassKind::Function as u32, false, true));
        assert!(!only_immutable.is_canonical() && only_immutable.is_immutable());

        // Neither flag may disturb the class id.
        for (c, i) in [(false, false), (true, false), (false, true), (true, true)] {
            let d = decode_tags(encode_tags(ClassKind::Library as u32, c, i));
            assert_eq!(d.get_cid(), ClassKind::Library as u32);
        }
    }

    #[test]
    fn application_class_ids_remain_valid_raw_cids() {
        let application_cid = ClassKind::NumPredefinedKinds as u32 + 37;
        let decoded = decode_tags(encode_tags(application_cid, false, false));
        assert_eq!(decoded.get_cid(), application_cid);
    }

    #[test]
    fn packed_tags_use_the_low_bit_for_canonical_and_have_no_immutable_flag() {
        for cid in [1, 32, 65, 0xfffff] {
            for canonical in [false, true] {
                let tags = (cid << 1) | canonical as u32;
                let decoded = decode_tags_for_kind(tags, ClassTagKind::PackedCidAndCanonical);
                assert_eq!(decoded.get_cid(), cid);
                assert_eq!(decoded.is_canonical(), canonical);
                assert!(!decoded.is_immutable());
            }
        }
    }

    #[test]
    fn header_tag_kinds_use_their_own_immutable_bit() {
        for cid in [1, 700, 0x80000, 0xfffff] {
            for canonical in [false, true] {
                for immutable_bits in [0, 1 << 6, 1 << 7, (1 << 6) | (1 << 7)] {
                    let tags = (cid << 12) | ((canonical as u32) << 1) | immutable_bits;
                    let current = decode_tags_for_kind(tags, ClassTagKind::ObjectHeader);
                    let deep =
                        decode_tags_for_kind(tags, ClassTagKind::ObjectHeaderDeeplyImmutable);
                    assert_eq!(current.get_cid(), cid);
                    assert_eq!(deep.get_cid(), cid);
                    assert_eq!(current.is_canonical(), canonical);
                    assert_eq!(deep.is_canonical(), canonical);
                    assert_eq!(current.is_immutable(), immutable_bits & (1 << 6) != 0);
                    assert_eq!(deep.is_immutable(), immutable_bits & (1 << 7) != 0);
                }
            }
        }
    }

    /// Real headers observed on the wire: the class id occupies bits 12..32, so
    /// a two-byte cid still round trips.
    #[test]
    fn wide_class_ids_survive_the_shift() {
        let d = decode_tags(encode_tags(
            ClassKind::NumPredefinedKinds as u32 - 1,
            false,
            false,
        ));
        assert_eq!(d.get_cid(), ClassKind::NumPredefinedKinds as u32 - 1);
    }
}
