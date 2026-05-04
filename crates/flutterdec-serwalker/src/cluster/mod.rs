use crate::constants::{ClassId, ClassId::*, SIGNED_M, UNSIGNED_M};
use crate::raw_object::*;
use crate::stream::Stream;
use crate::DECLARE_FIXED_LENGTH_CLUSTER;
use crate::DECLARE_VARIABLE_LENGTH_CLUSTER;
use crate::FFI_TYPES_LIST;

type Smi = i32;

pub trait Cluster {
    fn is_fixed_len(&self) -> bool;
    fn read_alloc(&mut self, last_ref_id: &mut u64, stream: &mut Stream) -> usize;
    fn read_fill(&mut self, stream: &mut Stream) -> usize;
}

pub fn read_smi(stream: &mut Stream) -> Smi {
    let raw_smi = stream.read_modified_leb128(SIGNED_M); // smis are always written as signed numbers

    raw_smi as Smi
}

macro_rules! FFI_CASE_PATTERN {
    ( $( $ffi_type:ident ),* ) => {
        $( $ffi_type )|*
    };
}

pub fn decide_cluster(class_id: ClassId) -> Result<Box<dyn Cluster>, &'static str> {
    match class_id {
        // we assume compressed pointers, it supports only Android for now...
        IllegalCid => Err("Not a supported class (illegal class)..."),
        FFI_TYPES_LIST!(FFI_CASE_PATTERN) => Err("To do..."),
        _ => Err("Not a supported class..."),
    }
}

// These are the objects that call ReadAllocFixedSize during deserialization,
// whose fill cluster size is uniquely determined by sizeof(Object) * num_of_objects
// and alloc cluster size is tags (MULEB128) + num_of_objects (MULEB128)

DECLARE_FIXED_LENGTH_CLUSTER!(TypeParameters<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(PatchClass<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(Function<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(ClosureData<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(FfiTrampolineData<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(Field<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(Script<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(Library<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(Namespace<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(KernelProgramInfo<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(UnlinkedCall, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(ICData, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(MegamorphicCache, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(SubtypeTestCache, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(LoadingUnit<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(LanguageError, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(UnhandledException, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(LibraryPrefix, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(Type<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(FunctionType<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(RecordType, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(TypeParameter<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(Closure<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(Double, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(Int32x4, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(GrowableObjectArray<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(TypedDataView<'a>, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(ExternalTypedData, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(StackTrace, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(RegExp, |_self, last_ref_id, stream| { 1 });
DECLARE_FIXED_LENGTH_CLUSTER!(WeakProperty, |_self, last_ref_id, stream| { 1 });

DECLARE_VARIABLE_LENGTH_CLUSTER!(Map);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Set);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Instance<'a>);
DECLARE_VARIABLE_LENGTH_CLUSTER!(TypedData<'a>);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Class<'a>);
DECLARE_VARIABLE_LENGTH_CLUSTER!(TypeArguments<'a>);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Code<'a>);
DECLARE_VARIABLE_LENGTH_CLUSTER!(ObjectPool);
DECLARE_VARIABLE_LENGTH_CLUSTER!(ExceptionHandlers<'a>);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Context<'a>);
DECLARE_VARIABLE_LENGTH_CLUSTER!(ContextScope);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Mint);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Float32x4);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Float64x2);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Record);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Array<'a>);
DECLARE_VARIABLE_LENGTH_CLUSTER!(WeakArray<'a>);
DECLARE_VARIABLE_LENGTH_CLUSTER!(ImmutableArray);
DECLARE_VARIABLE_LENGTH_CLUSTER!(ConstMap);
DECLARE_VARIABLE_LENGTH_CLUSTER!(ConstSet);
DECLARE_VARIABLE_LENGTH_CLUSTER!(CodeSourceMap);
DECLARE_VARIABLE_LENGTH_CLUSTER!(CompressedStackMaps);
DECLARE_VARIABLE_LENGTH_CLUSTER!(PcDescriptors);
DECLARE_VARIABLE_LENGTH_CLUSTER!(OneByteString);
DECLARE_VARIABLE_LENGTH_CLUSTER!(TwoByteString);
DECLARE_VARIABLE_LENGTH_CLUSTER!(_String);
