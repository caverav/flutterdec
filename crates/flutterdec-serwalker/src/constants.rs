use std::mem::size_of;

pub const MAGIC_BYTES: u32 = 0xdcdcf5f5;

pub const SNAPSHOT_MAGIC_NUMBER_SZ: usize = size_of::<u32>();
pub const SNAPSHOT_LEN_SZ: usize = size_of::<u64>();
pub const SNAPSHOT_KIND_SZ: usize = size_of::<u64>();

pub const SNAPSHOT_HEADER_SZ: usize = SNAPSHOT_MAGIC_NUMBER_SZ // 20 bytes of header
                                    + SNAPSHOT_LEN_SZ
                                    + SNAPSHOT_KIND_SZ;

pub const MAX_CLUSTER_NUM: usize = 67usize;

pub const UNSIGNED_END_OF_DATA_BYTE: u8 = 0x80u8; // last byte
pub const UNSIGNED_MAX_DATA_PER_BYTE: u8 = 0x7fu8; // more bytes to follow (for both)

pub const SIGNED_END_OF_DATA_BYTE: u8 = 0xc0u8; // last byte

pub const SIGNED_M: u8 = SIGNED_END_OF_DATA_BYTE;
pub const UNSIGNED_M: u8 = UNSIGNED_END_OF_DATA_BYTE;

pub const DATA_BITS_PER_BYTE: usize = 7usize;

pub const SMI_SHIFT: usize = 1usize;

pub const VERSION_HASH_LENGTH: usize = 32usize;

macro_rules! DEFINE_CLASS_ID {
    ( $( $name:ident = $val:expr ),* ) => {
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u32)]
        pub enum ClassId {
            #[default]
            IllegalCid = 0,
            $( $name = $val, )*
        }

        impl TryFrom<u32> for ClassId {
            type Error = &'static str;
            fn try_from(value: u32) -> Result<Self, Self::Error> {
                match value {
                    0 => Ok(ClassId::IllegalCid),
                    $( $val => Ok(ClassId::$name), )*
                    _ => Err("Invalid ClassId"),
                }
            }
        }
    };
}

DEFINE_CLASS_ID! {
    NativePointer = 1,
    FreeListElement = 2,
    ForwardingCorpse = 3,
    ObjectCid = 4,
    ClassCid = 5,
    PatchClassCid = 6,
    FunctionCid = 7,
    TypeParametersCid = 8,
    ClosureDataCid = 9,
    FfiTrampolineDataCid = 10,
    FieldCid = 11,
    ScriptCid = 12,
    LibraryCid = 13,
    NamespaceCid = 14,
    KernelProgramInfoCid = 15,
    WeakSerializationReferenceCid = 16,
    WeakArrayCid = 17,
    CodeCid = 18,
    BytecodeCid = 19,
    InstructionsCid = 20,
    InstructionsSectionCid = 21,
    InstructionsTableCid = 22,
    ObjectPoolCid = 23,
    PcDescriptorsCid = 24,
    CodeSourceMapCid = 25,
    CompressedStackMapsCid = 26,
    LocalVarDescriptorsCid = 27,
    ExceptionHandlersCid = 28,
    ContextCid = 29,
    ContextScopeCid = 30,
    SentinelCid = 31,
    SingleTargetCacheCid = 32,
    MonomorphicSmiableCallCid = 33,
    CallSiteDataCid = 34,
    UnlinkedCallCid = 35,
    ICDataCid = 36,
    MegamorphicCacheCid = 37,
    SubtypeTestCacheCid = 38,
    LoadingUnitCid = 39,
    ErrorCid = 40,
    ApiErrorCid = 41,
    LanguageErrorCid = 42,
    UnhandledExceptionCid = 43,
    UnwindErrorCid = 44,
    InstanceCid = 45,
    LibraryPrefixCid = 46,
    TypeArgumentsCid = 47,
    AbstractTypeCid = 48,
    TypeCid = 49,
    FunctionTypeCid = 50,
    RecordTypeCid = 51,
    TypeParameterCid = 52,
    FinalizerBaseCid = 53,
    FinalizerCid = 54,
    NativeFinalizerCid = 55,
    FinalizerEntryCid = 56,
    ClosureCid = 57,
    NumberCid = 58,
    IntegerCid = 59,
    SmiCid = 60,
    MintCid = 61,
    DoubleCid = 62,
    BoolCid = 63,
    Float32x4Cid = 64,
    Int32x4Cid = 65,
    Float64x2Cid = 66,
    RecordCid = 67,
    TypedDataBaseCid = 68,
    TypedDataCid = 69,
    ExternalTypedDataCid = 70,
    TypedDataViewCid = 71,
    PointerCid = 72,
    DynamicLibraryCid = 73,
    CapabilityCid = 74,
    ReceivePortCid = 75,
    SendPortCid = 76,
    StackTraceCid = 77,
    SuspendStateCid = 78,
    RegExpCid = 79,
    WeakPropertyCid = 80,
    WeakReferenceCid = 81,
    MirrorReferenceCid = 82,
    FutureOrCid = 83,
    UserTagCid = 84,
    TransferableTypedDataCid = 85,
    MapCid = 86,
    ConstMapCid = 87,
    SetCid = 88,
    ConstSetCid = 89,
    ArrayCid = 90,
    ImmutableArrayCid = 91,
    GrowableObjectArrayCid = 92,
    _StringCid = 93,
    OneByteStringCid = 94,
    TwoByteStringCid = 95,
    FfiNativeFunctionCid = 96,
    FfiInt8Cid = 97,
    FfiInt16Cid = 98,
    FfiInt32Cid = 99,
    FfiInt64Cid = 100,
    FfiUint8Cid = 101,
    FfiUint16Cid = 102,
    FfiUint32Cid = 103,
    FfiUint64Cid = 104,
    FfiFloatCid = 105,
    FfiDoubleCid = 106,
    FfiVoidCid = 107,
    FfiHandleCid = 108,
    FfiBoolCid = 109,
    FfiNativeTypeCid = 110,
    FfiStructCid = 111,
    TypedDataInt8ArrayCid = 112,
    TypedDataInt8ArrayViewCid = 113,
    ExternalTypedDataInt8ArrayCid = 114,
    UnmodifiableTypedDataInt8ArrayViewCid = 115,
    TypedDataUint8ArrayCid = 116,
    TypedDataUint8ArrayViewCid = 117,
    ExternalTypedDataUint8ArrayCid = 118,
    UnmodifiableTypedDataUint8ArrayViewCid = 119,
    TypedDataUint8ClampedArrayCid = 120,
    TypedDataUint8ClampedArrayViewCid = 121,
    ExternalTypedDataUint8ClampedArrayCid = 122,
    UnmodifiableTypedDataUint8ClampedArrayViewCid = 123,
    TypedDataInt16ArrayCid = 124,
    TypedDataInt16ArrayViewCid = 125,
    ExternalTypedDataInt16ArrayCid = 126,
    UnmodifiableTypedDataInt16ArrayViewCid = 127,
    TypedDataUint16ArrayCid = 128,
    TypedDataUint16ArrayViewCid = 129,
    ExternalTypedDataUint16ArrayCid = 130,
    UnmodifiableTypedDataUint16ArrayViewCid = 131,
    TypedDataInt32ArrayCid = 132,
    TypedDataInt32ArrayViewCid = 133,
    ExternalTypedDataInt32ArrayCid = 134,
    UnmodifiableTypedDataInt32ArrayViewCid = 135,
    TypedDataUint32ArrayCid = 136,
    TypedDataUint32ArrayViewCid = 137,
    ExternalTypedDataUint32ArrayCid = 138,
    UnmodifiableTypedDataUint32ArrayViewCid = 139,
    TypedDataInt64ArrayCid = 140,
    TypedDataInt64ArrayViewCid = 141,
    ExternalTypedDataInt64ArrayCid = 142,
    UnmodifiableTypedDataInt64ArrayViewCid = 143,
    TypedDataUint64ArrayCid = 144,
    TypedDataUint64ArrayViewCid = 145,
    ExternalTypedDataUint64ArrayCid = 146,
    UnmodifiableTypedDataUint64ArrayViewCid = 147,
    TypedDataFloat32ArrayCid = 148,
    TypedDataFloat32ArrayViewCid = 149,
    ExternalTypedDataFloat32ArrayCid = 150,
    UnmodifiableTypedDataFloat32ArrayViewCid = 151,
    TypedDataFloat64ArrayCid = 152,
    TypedDataFloat64ArrayViewCid = 153,
    ExternalTypedDataFloat64ArrayCid = 154,
    UnmodifiableTypedDataFloat64ArrayViewCid = 155,
    TypedDataFloat32x4ArrayCid = 156,
    TypedDataFloat32x4ArrayViewCid = 157,
    ExternalTypedDataFloat32x4ArrayCid = 158,
    UnmodifiableTypedDataFloat32x4ArrayViewCid = 159,
    TypedDataInt32x4ArrayCid = 160,
    TypedDataInt32x4ArrayViewCid = 161,
    ExternalTypedDataInt32x4ArrayCid = 162,
    UnmodifiableTypedDataInt32x4ArrayViewCid = 163,
    TypedDataFloat64x2ArrayCid = 164,
    TypedDataFloat64x2ArrayViewCid = 165,
    ExternalTypedDataFloat64x2ArrayCid = 166,
    UnmodifiableTypedDataFloat64x2ArrayViewCid = 167,
    ByteDataViewCid = 168,
    UnmodifiableByteDataViewCid = 169,
    ByteBufferCid = 170,
    NullCid = 171,
    DynamicCid = 172,
    VoidCid = 173,
    NeverCid = 174,
    NumPredefinedCids = 175
}

#[macro_export]
macro_rules! FFI_TYPES_LIST {
    ($callback:ident) => {
        $callback! {
            FfiNativeFunctionCid,
            FfiInt8Cid,
            FfiInt16Cid,
            FfiInt32Cid,
            FfiInt64Cid,
            FfiUint8Cid,
            FfiUint16Cid,
            FfiUint32Cid,
            FfiUint64Cid,
            FfiFloatCid,
            FfiDoubleCid,
            FfiVoidCid,
            FfiHandleCid,
            FfiBoolCid,
            FfiNativeTypeCid,
            FfiStructCid
        }
    };
}

/*


pub const NUM_BASE_OBJECTS_SZ: usize = size_of::<u64>();
pub const NUM_OBJECTS_SZ: usize = size_of::<u64>();
pub const NUM_CLUSTERS_SZ: usize = size_of::<u64>();

pub const INSTR_TABLE_LEN_SZ: usize = size_of::<u64>();
pub const INSTR_TABLE_OFFSET_SZ: usize = size_of::<u64>();

pub const CLUSTER_TAGS_SZ: usize = size_of::<u32>();
pub const CLUSTER_OBJ_COUNT_SZ: usize = size_of::<u64>();

pub const OBJECT_STORE_ENTRY_SIZE: usize = size_of::<u64>();
*/
