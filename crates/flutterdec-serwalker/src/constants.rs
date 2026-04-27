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

pub enum ClassId {
    IllegalCid = 0,
    NativePointer,
    FreeListElement,
    ForwardingCorpse,
    ObjectCid,
    ClassCid,
    PatchClassCid,
    FunctionCid,
    TypeParametersCid,
    ClosureDataCid,
    FfiTrampolineDataCid,
    FieldCid,
    ScriptCid,
    LibraryCid,
    NamespaceCid,
    KernelProgramInfoCid,
    WeakSerializationReferenceCid,
    WeakArrayCid,
    CodeCid,
    BytecodeCid,
    InstructionsCid,
    InstructionsSectionCid,
    InstructionsTableCid,
    ObjectPoolCid,
    PcDescriptorsCid,
    CodeSourceMapCid,
    CompressedStackMapsCid,
    LocalVarDescriptorsCid,
    ExceptionHandlersCid,
    ContextCid,
    ContextScopeCid,
    SentinelCid,
    SingleTargetCacheCid,
    MonomorphicSmiableCallCid,
    CallSiteDataCid,
    UnlinkedCallCid,
    ICDataCid,
    MegamorphicCacheCid,
    SubtypeTestCacheCid,
    LoadingUnitCid,
    ErrorCid,
    ApiErrorCid,
    LanguageErrorCid,
    UnhandledExceptionCid,
    UnwindErrorCid,
    InstanceCid,
    LibraryPrefixCid,
    TypeArgumentsCid,
    AbstractTypeCid,
    TypeCid,
    FunctionTypeCid,
    RecordTypeCid,
    TypeParameterCid,
    FinalizerBaseCid,
    FinalizerCid,
    NativeFinalizerCid,
    FinalizerEntryCid,
    ClosureCid,
    NumberCid,
    IntegerCid,
    SmiCid,
    MintCid,
    DoubleCid,
    BoolCid,
    Float32x4Cid,
    Int32x4Cid,
    Float64x2Cid,
    RecordCid,
    TypedDataBaseCid,
    TypedDataCid,
    ExternalTypedDataCid,
    TypedDataViewCid,
    PointerCid,
    DynamicLibraryCid,
    CapabilityCid,
    ReceivePortCid,
    SendPortCid,
    StackTraceCid,
    SuspendStateCid,
    RegExpCid,
    WeakPropertyCid,
    WeakReferenceCid,
    MirrorReferenceCid,
    FutureOrCid,
    UserTagCid,
    TransferableTypedDataCid,
    MapCid,
    ConstMapCid,
    SetCid,
    ConstSetCid,
    ArrayCid,
    ImmutableArrayCid,
    GrowableObjectArrayCid,
    StringCid,
    OneByteStringCid,
    TwoByteStringCid,
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
    FfiStructCid,
    TypedDataInt8ArrayCid,
    TypedDataInt8ArrayViewCid,
    ExternalTypedDataInt8ArrayCid,
    UnmodifiableTypedDataInt8ArrayViewCid,
    TypedDataUint8ArrayCid,
    TypedDataUint8ArrayViewCid,
    ExternalTypedDataUint8ArrayCid,
    UnmodifiableTypedDataUint8ArrayViewCid,
    TypedDataUint8ClampedArrayCid,
    TypedDataUint8ClampedArrayViewCid,
    ExternalTypedDataUint8ClampedArrayCid,
    UnmodifiableTypedDataUint8ClampedArrayViewCid,
    TypedDataInt16ArrayCid,
    TypedDataInt16ArrayViewCid,
    ExternalTypedDataInt16ArrayCid,
    UnmodifiableTypedDataInt16ArrayViewCid,
    TypedDataUint16ArrayCid,
    TypedDataUint16ArrayViewCid,
    ExternalTypedDataUint16ArrayCid,
    UnmodifiableTypedDataUint16ArrayViewCid,
    TypedDataInt32ArrayCid,
    TypedDataInt32ArrayViewCid,
    ExternalTypedDataInt32ArrayCid,
    UnmodifiableTypedDataInt32ArrayViewCid,
    TypedDataUint32ArrayCid,
    TypedDataUint32ArrayViewCid,
    ExternalTypedDataUint32ArrayCid,
    UnmodifiableTypedDataUint32ArrayViewCid,
    TypedDataInt64ArrayCid,
    TypedDataInt64ArrayViewCid,
    ExternalTypedDataInt64ArrayCid,
    UnmodifiableTypedDataInt64ArrayViewCid,
    TypedDataUint64ArrayCid,
    TypedDataUint64ArrayViewCid,
    ExternalTypedDataUint64ArrayCid,
    UnmodifiableTypedDataUint64ArrayViewCid,
    TypedDataFloat32ArrayCid,
    TypedDataFloat32ArrayViewCid,
    ExternalTypedDataFloat32ArrayCid,
    UnmodifiableTypedDataFloat32ArrayViewCid,
    TypedDataFloat64ArrayCid,
    TypedDataFloat64ArrayViewCid,
    ExternalTypedDataFloat64ArrayCid,
    UnmodifiableTypedDataFloat64ArrayViewCid,
    TypedDataFloat32x4ArrayCid,
    TypedDataFloat32x4ArrayViewCid,
    ExternalTypedDataFloat32x4ArrayCid,
    UnmodifiableTypedDataFloat32x4ArrayViewCid,
    TypedDataInt32x4ArrayCid,
    TypedDataInt32x4ArrayViewCid,
    ExternalTypedDataInt32x4ArrayCid,
    UnmodifiableTypedDataInt32x4ArrayViewCid,
    TypedDataFloat64x2ArrayCid,
    TypedDataFloat64x2ArrayViewCid,
    ExternalTypedDataFloat64x2ArrayCid,
    UnmodifiableTypedDataFloat64x2ArrayViewCid,
    ByteDataViewCid,
    UnmodifiableByteDataViewCid,
    ByteBufferCid,
    NullCid,
    DynamicCid,
    VoidCid,
    NeverCid,
    NumPredefinedCids,
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
