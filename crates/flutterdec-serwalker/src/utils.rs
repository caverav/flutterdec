use crate::constants::ClassId;

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
        pub struct $cluster_name {
            tags: u32,
            cid: ClassId,
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

            fn set_metadata(
                &mut self,
                tags: u32,
                cid: ClassId,
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
        pub struct $cluster_name {
            tags: u32,
            cid: ClassId,
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
    class_id: ClassId,
    is_immutable: bool,
    is_canonical: bool,
}

impl DecodedTags {
    pub fn new(cid: ClassId, immut: bool, canonical: bool) -> Self {
        Self {
            class_id: cid,
            is_immutable: immut,
            is_canonical: canonical,
        }
    }

    pub fn get_cid(&self) -> ClassId {
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
        ClassId::try_from(($tags >> 12) & 0xFFFFF)
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

pub fn decode_tags(tags: u32) -> anyhow::Result<DecodedTags> {
    let class_id = DECODE_CID!(tags).map_err(|_| {
        anyhow::anyhow!(
            "unknown class id {} in tags {tags:#x}",
            (tags >> 12) & 0xFFFFF
        )
    })?;
    Ok(DecodedTags::new(
        class_id,
        DECODE_IS_IMMUTABLE!(tags),
        DECODE_IS_CANONICAL!(tags),
    ))
}
