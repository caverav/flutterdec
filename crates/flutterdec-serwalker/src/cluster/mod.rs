use crate::constants::{ClassId, ClassId::*, SIGNED_M, UNSIGNED_M};
use crate::raw_object::*;
use crate::stream::Stream;
use crate::DECLARE_FIXED_LENGTH_CLUSTER;
use crate::FFI_TYPES_LIST;

type Smi = i32;

pub trait Cluster {
    fn is_fixed_len(&self) -> bool;
    fn read_alloc(&mut self, last_ref_id: &mut u64, stream: &mut Stream) -> usize;
    fn read_fill(&mut self, last_ref_id: &mut u64, stream: &mut Stream) -> usize;
}

pub fn read_cluster_alloc() {
    //let curr_ref_id: u64 = 0;
}

pub fn read_cluster_fill() {
    //let curr_ref_id: u64 = 0;
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

DECLARE_FIXED_LENGTH_CLUSTER!(TypeParameters, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(PatchClass, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(Function, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(ClosureData, {
    1
    // to-do
});
DECLARE_FIXED_LENGTH_CLUSTER!(FfiTrampolineData, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(Field, {
    1
    // to-do
});
DECLARE_FIXED_LENGTH_CLUSTER!(Script, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(Library, {
    1
    // to-do
});
DECLARE_FIXED_LENGTH_CLUSTER!(Namespace, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(KernelProgramInfo, {
    1
    // to-do
});
DECLARE_FIXED_LENGTH_CLUSTER!(UnlinkedCall, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(ICData, {
    1
    // to-do
});
DECLARE_FIXED_LENGTH_CLUSTER!(MegamorphicCache, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(SubtypeTestCache, {
    1
    // to-do
});
DECLARE_FIXED_LENGTH_CLUSTER!(LoadingUnit, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(LanguageError, {
    1
    // to-do
});
DECLARE_FIXED_LENGTH_CLUSTER!(UnhandledException, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(LibraryPrefix, {
    1
    // to-do
});
DECLARE_FIXED_LENGTH_CLUSTER!(Type, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(FunctionType, {
    1
    // to-do
});
DECLARE_FIXED_LENGTH_CLUSTER!(RecordType, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(TypeParameter, {
    1
    // to-do
});
DECLARE_FIXED_LENGTH_CLUSTER!(Closure, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(Double, {
    1
    // to-do
});
DECLARE_FIXED_LENGTH_CLUSTER!(Int32x4, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(GrowableObjectArray, {
    1
    // to-do
});
DECLARE_FIXED_LENGTH_CLUSTER!(TypedDataView, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(ExternalTypedData, {
    1
    // to-do
});
DECLARE_FIXED_LENGTH_CLUSTER!(StackTrace, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(RegExp, {
    1
    // to-do
});
DECLARE_FIXED_LENGTH_CLUSTER!(WeakProperty, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(Map, {
    1
    // to-do
});

DECLARE_FIXED_LENGTH_CLUSTER!(Set, {
    1
    // to-do
});
