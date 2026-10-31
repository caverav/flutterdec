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

// Version::SnapshotString() for the Dart SDK 3.11.1 tag (e927f58e327a).
// The current parser has a hardcoded VM schema, so reject other layouts until
// schema information can be supplied dynamically.
pub const DART_3_11_1_SNAPSHOT_HASH: &str = "78da37fed6bf1489361a312568249f3f";

pub const OBJECT_START_ALIGNMENT: usize = 64;

pub type Cid = u32;

macro_rules! DEFINE_CLASS_KIND {
    ( $( $name:ident = $val:expr ),* ) => {
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u32)]
        pub enum ClassKind {
            #[default]
            Illegal = 0,
            $( $name = $val, )*
        }
    };
}

// These class kinds do NO longer represent real Dart class IDs, like they do in the first branch.
// The numbering and ordering is based on Dart 3.11.1, but this enum is now used as a stable discrete dictionary
// for the rest of the application instead of being actual hardcoded Dart class IDs. This means that, for example, components such as the info_producer
// do not need to be aware of the real class ID a certain snapshot hash defines for a certain class kind, it only has to make use of the
// ClassKind it needs and the Snapshot translator (cid_for_class_kind) will do the translation. This is a golden rule from now on for anyone who wants to contribute.
DEFINE_CLASS_KIND! {
    NativePointer = 1,
    FreeListElement = 2,
    ForwardingCorpse = 3,
    Object = 4,
    Class = 5,
    PatchClass = 6,
    Function = 7,
    TypeParameters = 8,
    ClosureData = 9,
    FfiTrampolineData = 10,
    Field = 11,
    Script = 12,
    Library = 13,
    Namespace = 14,
    KernelProgramInfo = 15,
    WeakSerializationReference = 16,
    WeakArray = 17,
    Code = 18,
    Bytecode = 19,
    Instructions = 20,
    InstructionsSection = 21,
    InstructionsTable = 22,
    ObjectPool = 23,
    PcDescriptors = 24,
    CodeSourceMap = 25,
    CompressedStackMaps = 26,
    LocalVarDescriptors = 27,
    ExceptionHandlers = 28,
    Context = 29,
    ContextScope = 30,
    Sentinel = 31,
    SingleTargetCache = 32,
    MonomorphicSmiableCall = 33,
    CallSiteData = 34,
    UnlinkedCall = 35,
    ICData = 36,
    MegamorphicCache = 37,
    SubtypeTestCache = 38,
    LoadingUnit = 39,
    Error = 40,
    ApiError = 41,
    LanguageError = 42,
    UnhandledException = 43,
    UnwindError = 44,
    Instance = 45,
    LibraryPrefix = 46,
    TypeArguments = 47,
    AbstractType = 48,
    Type = 49,
    FunctionType = 50,
    RecordType = 51,
    TypeParameter = 52,
    FinalizerBase = 53,
    Finalizer = 54,
    NativeFinalizer = 55,
    FinalizerEntry = 56,
    Closure = 57,
    Number = 58,
    Integer = 59,
    Smi = 60,
    Mint = 61,
    Double = 62,
    Bool = 63,
    Float32x4 = 64,
    Int32x4 = 65,
    Float64x2 = 66,
    Record = 67,
    TypedDataBase = 68,
    TypedData = 69,
    ExternalTypedData = 70,
    TypedDataView = 71,
    Pointer = 72,
    DynamicLibrary = 73,
    Capability = 74,
    ReceivePort = 75,
    SendPort = 76,
    StackTrace = 77,
    SuspendState = 78,
    RegExp = 79,
    WeakProperty = 80,
    WeakReference = 81,
    MirrorReference = 82,
    FutureOr = 83,
    UserTag = 84,
    TransferableTypedData = 85,
    Map = 86,
    ConstMap = 87,
    Set = 88,
    ConstSet = 89,
    Array = 90,
    ImmutableArray = 91,
    GrowableObjectArray = 92,
    _String = 93,
    OneByteString = 94,
    TwoByteString = 95,
    FfiNativeFunction = 96,
    FfiInt8 = 97,
    FfiInt16 = 98,
    FfiInt32 = 99,
    FfiInt64 = 100,
    FfiUint8 = 101,
    FfiUint16 = 102,
    FfiUint32 = 103,
    FfiUint64 = 104,
    FfiFloat = 105,
    FfiDouble = 106,
    FfiVoid = 107,
    FfiHandle = 108,
    FfiBool = 109,
    FfiNativeType = 110,
    FfiStruct = 111,
    TypedDataInt8Array = 112,
    TypedDataInt8ArrayView = 113,
    ExternalTypedDataInt8Array = 114,
    UnmodifiableTypedDataInt8ArrayView = 115,
    TypedDataUint8Array = 116,
    TypedDataUint8ArrayView = 117,
    ExternalTypedDataUint8Array = 118,
    UnmodifiableTypedDataUint8ArrayView = 119,
    TypedDataUint8ClampedArray = 120,
    TypedDataUint8ClampedArrayView = 121,
    ExternalTypedDataUint8ClampedArray = 122,
    UnmodifiableTypedDataUint8ClampedArrayView = 123,
    TypedDataInt16Array = 124,
    TypedDataInt16ArrayView = 125,
    ExternalTypedDataInt16Array = 126,
    UnmodifiableTypedDataInt16ArrayView = 127,
    TypedDataUint16Array = 128,
    TypedDataUint16ArrayView = 129,
    ExternalTypedDataUint16Array = 130,
    UnmodifiableTypedDataUint16ArrayView = 131,
    TypedDataInt32Array = 132,
    TypedDataInt32ArrayView = 133,
    ExternalTypedDataInt32Array = 134,
    UnmodifiableTypedDataInt32ArrayView = 135,
    TypedDataUint32Array = 136,
    TypedDataUint32ArrayView = 137,
    ExternalTypedDataUint32Array = 138,
    UnmodifiableTypedDataUint32ArrayView = 139,
    TypedDataInt64Array = 140,
    TypedDataInt64ArrayView = 141,
    ExternalTypedDataInt64Array = 142,
    UnmodifiableTypedDataInt64ArrayView = 143,
    TypedDataUint64Array = 144,
    TypedDataUint64ArrayView = 145,
    ExternalTypedDataUint64Array = 146,
    UnmodifiableTypedDataUint64ArrayView = 147,
    TypedDataFloat32Array = 148,
    TypedDataFloat32ArrayView = 149,
    ExternalTypedDataFloat32Array = 150,
    UnmodifiableTypedDataFloat32ArrayView = 151,
    TypedDataFloat64Array = 152,
    TypedDataFloat64ArrayView = 153,
    ExternalTypedDataFloat64Array = 154,
    UnmodifiableTypedDataFloat64ArrayView = 155,
    TypedDataFloat32x4Array = 156,
    TypedDataFloat32x4ArrayView = 157,
    ExternalTypedDataFloat32x4Array = 158,
    UnmodifiableTypedDataFloat32x4ArrayView = 159,
    TypedDataInt32x4Array = 160,
    TypedDataInt32x4ArrayView = 161,
    ExternalTypedDataInt32x4Array = 162,
    UnmodifiableTypedDataInt32x4ArrayView = 163,
    TypedDataFloat64x2Array = 164,
    TypedDataFloat64x2ArrayView = 165,
    ExternalTypedDataFloat64x2Array = 166,
    UnmodifiableTypedDataFloat64x2ArrayView = 167,
    ByteDataView = 168,
    UnmodifiableByteDataView = 169,
    ByteBuffer = 170,
    Null = 171,
    Dynamic = 172,
    Void = 173,
    Never = 174,
    NumPredefinedKinds = 175
}

#[macro_export]
macro_rules! FFI_TYPES_LIST {
    ($callback:ident) => {
        $callback! {
            FfiInt8,
            FfiInt16,
            FfiInt32,
            FfiInt64,
            FfiUint8,
            FfiUint16,
            FfiUint32,
            FfiUint64,
            FfiFloat,
            FfiDouble,
            FfiVoid,
            FfiHandle,
            FfiBool
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
