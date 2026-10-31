use crate::{
    constants::ClassKind,
    profile_reader::{
        profile::{ClassTagKind, SnapshotArchitecture, SnapshotFeatures},
        profile_json_dictionary::*,
    },
};
use anyhow::Context;
use serde_json;
use std::{collections::HashMap, env, fs};

pub fn features_suffix(features: &Vec<SnapshotFeatures>) -> String {
    let mut feature_string = String::new();

    for (idx, feature) in features.iter().enumerate() {
        match feature {
            SnapshotFeatures::CompressedPointers => feature_string += "compr",
            // maybe add more in the future, for now, its just this
        }

        if idx == features.len() - 1 {
            break;
        }

        feature_string += "-";
    }

    feature_string
}

pub fn architecture_suffix(arch: &SnapshotArchitecture) -> String {
    let arch_string = match arch {
        SnapshotArchitecture::ArchARM => String::from("arm"),
        SnapshotArchitecture::ArchRISCV32 => String::from("riscv32"),
        SnapshotArchitecture::ArchRISCV64 => String::from("riscv64"),
        SnapshotArchitecture::ArchX64 => String::from("x64"),
        SnapshotArchitecture::ArchX86 => String::from("ia32"),
        SnapshotArchitecture::ArchArm64 => String::from("arm64"),
    };

    arch_string
}

pub(super) fn resolve_snapshot_architecture(
    features_string: &Vec<String>,
) -> anyhow::Result<SnapshotArchitecture> {
    for feature in features_string {
        if let Ok(arch) = SnapshotArchitecture::try_from(feature) {
            return Ok(arch);
        }
    }

    Err(anyhow::anyhow!(
        "Architecture not found in the features string..."
    ))
}

const PROFILE_PATH_ENV_VAR_NAME: &str = "FLUTTERDEC_PROFILES_DIR";

// read the data_profile file and return an (ops, cid map, num predefined cids, inverse cid map) tuple
pub(super) fn read_profile_file(
    file_name: &String,
) -> anyhow::Result<(
    HashMap<String, serde_json::Value>,
    HashMap<u32, ClassKind>,
    u32,
    HashMap<ClassKind, u32>,
    ClassTagKind,
)> {
    let profile_file: Vec<u8>;

    if let Ok(profiles_dirpath) = env::var(PROFILE_PATH_ENV_VAR_NAME) {
        let full_path = profiles_dirpath + "/" + file_name;
        profile_file =
            fs::read(&full_path).with_context(|| format!("read data profile: {}", full_path))?;
    } else {
        // fallback #1 try reading the working directory
        profile_file = fs::read(file_name).with_context(|| {
            format!("read data profile from current working directory: {file_name}")
        })?;
    }

    let mut profile = serde_json::from_slice::<HashMap<String, serde_json::Value>>(&profile_file)?;
    let operations_value = profile.remove(PARSING_OPERATIONS).ok_or_else(|| {
        anyhow::anyhow!("Couldn't recover the parsing operations from the profile file.")
    })?;
    let ops = serde_json::from_value::<HashMap<String, serde_json::Value>>(operations_value)?;

    let class_map_value = profile
        .remove(CID_MAP)
        .ok_or_else(|| anyhow::anyhow!("Couldn't recover the CID map from the profile file."))?;
    let class_names = serde_json::from_value::<HashMap<u32, String>>(class_map_value)?;

    let num_predefined_cids_value = profile.remove(NUM_PREDEFINED_CIDS_KEY).ok_or_else(|| {
        anyhow::anyhow!("Couldn't recover num_predefined_cids from the profile file.")
    })?;
    let num_predefined_cids = serde_json::from_value::<u32>(num_predefined_cids_value)?;
    let class_tag_kind =
        serde_json::from_value::<ClassTagKind>(profile.remove(CLASS_TAG_KIND_KEY).ok_or_else(
            || anyhow::anyhow!("Couldn't recover class_tag_kind from the profile file."),
        )?)?;

    let mut cid_map = HashMap::with_capacity(class_names.len());
    let mut class_kind_to_cid = HashMap::with_capacity(class_names.len());
    for (raw_cid, class_name) in class_names {
        let class_kind = class_name_txlate(&class_name);
        anyhow::ensure!(
            class_kind != ClassKind::Illegal || class_name == ILLEGAL,
            "unknown class name {class_name:?} for CID {raw_cid} in profile"
        );
        if let Some(previous_cid) = class_kind_to_cid.insert(class_kind, raw_cid) {
            anyhow::bail!(
                "profile maps {class_name:?} to both CID {previous_cid} and CID {raw_cid}"
            );
        }
        cid_map.insert(raw_cid, class_kind);
    }

    Ok((
        ops,
        cid_map,
        num_predefined_cids,
        class_kind_to_cid,
        class_tag_kind,
    ))
}

// Strictly speaking, this function is not required. We could change decide_cluster to use the &str constants
// defined in profile_json_dictioanry.rs, but this seemingly pointless mapping exists to give the profile data
// transparency from the other components viewpoints, this is, the info_producer or other operations that operate on
// object data obtained during cluster deserialization do NOT need to know specifics or any detail whatsoever defined
// in the data profile. Instead, those details are abstracted away via mapping them to stable constants or states. See the
// comment for the DEFINE_CLASS_KIND! macro in src/constants.rs to know more.
pub fn class_name_txlate(class_name: &str) -> ClassKind {
    match class_name {
        ILLEGAL => ClassKind::Illegal,
        NATIVE_POINTER => ClassKind::NativePointer,
        FREE_LIST_ELEMENT => ClassKind::FreeListElement,
        FORWARDING_CORPSE => ClassKind::ForwardingCorpse,
        OBJECT => ClassKind::Object,
        CLASS => ClassKind::Class,
        PATCH_CLASS => ClassKind::PatchClass,
        FUNCTION => ClassKind::Function,
        TYPE_PARAMETERS => ClassKind::TypeParameters,
        CLOSURE_DATA => ClassKind::ClosureData,
        FFI_TRAMPOLINE_DATA => ClassKind::FfiTrampolineData,
        FIELD => ClassKind::Field,
        SCRIPT => ClassKind::Script,
        LIBRARY => ClassKind::Library,
        NAMESPACE => ClassKind::Namespace,
        KERNEL_PROGRAM_INFO => ClassKind::KernelProgramInfo,
        WEAK_SERIALIZATION_REFERENCE => ClassKind::WeakSerializationReference,
        WEAK_ARRAY => ClassKind::WeakArray,
        CODE => ClassKind::Code,
        BYTECODE => ClassKind::Bytecode,
        INSTRUCTIONS => ClassKind::Instructions,
        INSTRUCTIONS_SECTION => ClassKind::InstructionsSection,
        INSTRUCTIONS_TABLE => ClassKind::InstructionsTable,
        OBJECT_POOL => ClassKind::ObjectPool,
        PC_DESCRIPTORS => ClassKind::PcDescriptors,
        CODE_SOURCE_MAP => ClassKind::CodeSourceMap,
        COMPRESSED_STACK_MAPS => ClassKind::CompressedStackMaps,
        LOCAL_VAR_DESCRIPTORS => ClassKind::LocalVarDescriptors,
        EXCEPTION_HANDLERS => ClassKind::ExceptionHandlers,
        CONTEXT => ClassKind::Context,
        CONTEXT_SCOPE => ClassKind::ContextScope,
        SENTINEL => ClassKind::Sentinel,
        SINGLE_TARGET_CACHE => ClassKind::SingleTargetCache,
        MONOMORPHIC_SMIABLE_CALL => ClassKind::MonomorphicSmiableCall,
        CALL_SITE_DATA => ClassKind::CallSiteData,
        UNLINKED_CALL => ClassKind::UnlinkedCall,
        IC_DATA => ClassKind::ICData,
        MEGAMORPHIC_CACHE => ClassKind::MegamorphicCache,
        SUBTYPE_TEST_CACHE => ClassKind::SubtypeTestCache,
        LOADING_UNIT => ClassKind::LoadingUnit,
        ERROR => ClassKind::Error,
        API_ERROR => ClassKind::ApiError,
        LANGUAGE_ERROR => ClassKind::LanguageError,
        UNHANDLED_EXCEPTION => ClassKind::UnhandledException,
        UNWIND_ERROR => ClassKind::UnwindError,
        INSTANCE => ClassKind::Instance,
        LIBRARY_PREFIX => ClassKind::LibraryPrefix,
        TYPE_ARGUMENTS => ClassKind::TypeArguments,
        ABSTRACT_TYPE => ClassKind::AbstractType,
        TYPE => ClassKind::Type,
        FUNCTION_TYPE => ClassKind::FunctionType,
        RECORD_TYPE => ClassKind::RecordType,
        TYPE_PARAMETER => ClassKind::TypeParameter,
        FINALIZER_BASE => ClassKind::FinalizerBase,
        FINALIZER => ClassKind::Finalizer,
        NATIVE_FINALIZER => ClassKind::NativeFinalizer,
        FINALIZER_ENTRY => ClassKind::FinalizerEntry,
        CLOSURE => ClassKind::Closure,
        NUMBER => ClassKind::Number,
        INTEGER => ClassKind::Integer,
        SMI => ClassKind::Smi,
        MINT => ClassKind::Mint,
        DOUBLE => ClassKind::Double,
        BOOL => ClassKind::Bool,
        FLOAT32X4 => ClassKind::Float32x4,
        INT32X4 => ClassKind::Int32x4,
        FLOAT64X2 => ClassKind::Float64x2,
        RECORD => ClassKind::Record,
        TYPED_DATA_BASE => ClassKind::TypedDataBase,
        TYPED_DATA => ClassKind::TypedData,
        EXTERNAL_TYPED_DATA => ClassKind::ExternalTypedData,
        TYPED_DATA_VIEW => ClassKind::TypedDataView,
        POINTER => ClassKind::Pointer,
        DYNAMIC_LIBRARY => ClassKind::DynamicLibrary,
        CAPABILITY => ClassKind::Capability,
        RECEIVE_PORT => ClassKind::ReceivePort,
        SEND_PORT => ClassKind::SendPort,
        STACK_TRACE => ClassKind::StackTrace,
        SUSPEND_STATE => ClassKind::SuspendState,
        REG_EXP => ClassKind::RegExp,
        WEAK_PROPERTY => ClassKind::WeakProperty,
        WEAK_REFERENCE => ClassKind::WeakReference,
        MIRROR_REFERENCE => ClassKind::MirrorReference,
        FUTURE_OR => ClassKind::FutureOr,
        USER_TAG => ClassKind::UserTag,
        TRANSFERABLE_TYPED_DATA => ClassKind::TransferableTypedData,
        MAP => ClassKind::Map,
        CONST_MAP => ClassKind::ConstMap,
        SET => ClassKind::Set,
        CONST_SET => ClassKind::ConstSet,
        ARRAY => ClassKind::Array,
        IMMUTABLE_ARRAY => ClassKind::ImmutableArray,
        GROWABLE_OBJECT_ARRAY => ClassKind::GrowableObjectArray,
        STRING => ClassKind::_String,
        ONE_BYTE_STRING => ClassKind::OneByteString,
        TWO_BYTE_STRING => ClassKind::TwoByteString,
        FFI_NATIVE_FUNCTION => ClassKind::FfiNativeFunction,
        FFI_INT8 => ClassKind::FfiInt8,
        FFI_INT16 => ClassKind::FfiInt16,
        FFI_INT32 => ClassKind::FfiInt32,
        FFI_INT64 => ClassKind::FfiInt64,
        FFI_UINT8 => ClassKind::FfiUint8,
        FFI_UINT16 => ClassKind::FfiUint16,
        FFI_UINT32 => ClassKind::FfiUint32,
        FFI_UINT64 => ClassKind::FfiUint64,
        FFI_FLOAT => ClassKind::FfiFloat,
        FFI_DOUBLE => ClassKind::FfiDouble,
        FFI_VOID => ClassKind::FfiVoid,
        FFI_HANDLE => ClassKind::FfiHandle,
        FFI_BOOL => ClassKind::FfiBool,
        FFI_NATIVE_TYPE => ClassKind::FfiNativeType,
        FFI_STRUCT => ClassKind::FfiStruct,
        TYPED_DATA_INT8_ARRAY => ClassKind::TypedDataInt8Array,
        TYPED_DATA_INT8_ARRAY_VIEW => ClassKind::TypedDataInt8ArrayView,
        EXTERNAL_TYPED_DATA_INT8_ARRAY => ClassKind::ExternalTypedDataInt8Array,
        UNMODIFIABLE_TYPED_DATA_INT8_ARRAY_VIEW => ClassKind::UnmodifiableTypedDataInt8ArrayView,
        TYPED_DATA_UINT8_ARRAY => ClassKind::TypedDataUint8Array,
        TYPED_DATA_UINT8_ARRAY_VIEW => ClassKind::TypedDataUint8ArrayView,
        EXTERNAL_TYPED_DATA_UINT8_ARRAY => ClassKind::ExternalTypedDataUint8Array,
        UNMODIFIABLE_TYPED_DATA_UINT8_ARRAY_VIEW => ClassKind::UnmodifiableTypedDataUint8ArrayView,
        TYPED_DATA_UINT8_CLAMPED_ARRAY => ClassKind::TypedDataUint8ClampedArray,
        TYPED_DATA_UINT8_CLAMPED_ARRAY_VIEW => ClassKind::TypedDataUint8ClampedArrayView,
        EXTERNAL_TYPED_DATA_UINT8_CLAMPED_ARRAY => ClassKind::ExternalTypedDataUint8ClampedArray,
        UNMODIFIABLE_TYPED_DATA_UINT8_CLAMPED_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataUint8ClampedArrayView
        }
        TYPED_DATA_INT16_ARRAY => ClassKind::TypedDataInt16Array,
        TYPED_DATA_INT16_ARRAY_VIEW => ClassKind::TypedDataInt16ArrayView,
        EXTERNAL_TYPED_DATA_INT16_ARRAY => ClassKind::ExternalTypedDataInt16Array,
        UNMODIFIABLE_TYPED_DATA_INT16_ARRAY_VIEW => ClassKind::UnmodifiableTypedDataInt16ArrayView,
        TYPED_DATA_UINT16_ARRAY => ClassKind::TypedDataUint16Array,
        TYPED_DATA_UINT16_ARRAY_VIEW => ClassKind::TypedDataUint16ArrayView,
        EXTERNAL_TYPED_DATA_UINT16_ARRAY => ClassKind::ExternalTypedDataUint16Array,
        UNMODIFIABLE_TYPED_DATA_UINT16_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataUint16ArrayView
        }
        TYPED_DATA_INT32_ARRAY => ClassKind::TypedDataInt32Array,
        TYPED_DATA_INT32_ARRAY_VIEW => ClassKind::TypedDataInt32ArrayView,
        EXTERNAL_TYPED_DATA_INT32_ARRAY => ClassKind::ExternalTypedDataInt32Array,
        UNMODIFIABLE_TYPED_DATA_INT32_ARRAY_VIEW => ClassKind::UnmodifiableTypedDataInt32ArrayView,
        TYPED_DATA_UINT32_ARRAY => ClassKind::TypedDataUint32Array,
        TYPED_DATA_UINT32_ARRAY_VIEW => ClassKind::TypedDataUint32ArrayView,
        EXTERNAL_TYPED_DATA_UINT32_ARRAY => ClassKind::ExternalTypedDataUint32Array,
        UNMODIFIABLE_TYPED_DATA_UINT32_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataUint32ArrayView
        }
        TYPED_DATA_INT64_ARRAY => ClassKind::TypedDataInt64Array,
        TYPED_DATA_INT64_ARRAY_VIEW => ClassKind::TypedDataInt64ArrayView,
        EXTERNAL_TYPED_DATA_INT64_ARRAY => ClassKind::ExternalTypedDataInt64Array,
        UNMODIFIABLE_TYPED_DATA_INT64_ARRAY_VIEW => ClassKind::UnmodifiableTypedDataInt64ArrayView,
        TYPED_DATA_UINT64_ARRAY => ClassKind::TypedDataUint64Array,
        TYPED_DATA_UINT64_ARRAY_VIEW => ClassKind::TypedDataUint64ArrayView,
        EXTERNAL_TYPED_DATA_UINT64_ARRAY => ClassKind::ExternalTypedDataUint64Array,
        UNMODIFIABLE_TYPED_DATA_UINT64_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataUint64ArrayView
        }
        TYPED_DATA_FLOAT32_ARRAY => ClassKind::TypedDataFloat32Array,
        TYPED_DATA_FLOAT32_ARRAY_VIEW => ClassKind::TypedDataFloat32ArrayView,
        EXTERNAL_TYPED_DATA_FLOAT32_ARRAY => ClassKind::ExternalTypedDataFloat32Array,
        UNMODIFIABLE_TYPED_DATA_FLOAT32_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataFloat32ArrayView
        }
        TYPED_DATA_FLOAT64_ARRAY => ClassKind::TypedDataFloat64Array,
        TYPED_DATA_FLOAT64_ARRAY_VIEW => ClassKind::TypedDataFloat64ArrayView,
        EXTERNAL_TYPED_DATA_FLOAT64_ARRAY => ClassKind::ExternalTypedDataFloat64Array,
        UNMODIFIABLE_TYPED_DATA_FLOAT64_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataFloat64ArrayView
        }
        TYPED_DATA_FLOAT32X4_ARRAY => ClassKind::TypedDataFloat32x4Array,
        TYPED_DATA_FLOAT32X4_ARRAY_VIEW => ClassKind::TypedDataFloat32x4ArrayView,
        EXTERNAL_TYPED_DATA_FLOAT32X4_ARRAY => ClassKind::ExternalTypedDataFloat32x4Array,
        UNMODIFIABLE_TYPED_DATA_FLOAT32X4_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataFloat32x4ArrayView
        }
        TYPED_DATA_INT32X4_ARRAY => ClassKind::TypedDataInt32x4Array,
        TYPED_DATA_INT32X4_ARRAY_VIEW => ClassKind::TypedDataInt32x4ArrayView,
        EXTERNAL_TYPED_DATA_INT32X4_ARRAY => ClassKind::ExternalTypedDataInt32x4Array,
        UNMODIFIABLE_TYPED_DATA_INT32X4_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataInt32x4ArrayView
        }
        TYPED_DATA_FLOAT64X2_ARRAY => ClassKind::TypedDataFloat64x2Array,
        TYPED_DATA_FLOAT64X2_ARRAY_VIEW => ClassKind::TypedDataFloat64x2ArrayView,
        EXTERNAL_TYPED_DATA_FLOAT64X2_ARRAY => ClassKind::ExternalTypedDataFloat64x2Array,
        UNMODIFIABLE_TYPED_DATA_FLOAT64X2_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataFloat64x2ArrayView
        }
        BYTE_DATA_VIEW => ClassKind::ByteDataView,
        UNMODIFIABLE_BYTE_DATA_VIEW => ClassKind::UnmodifiableByteDataView,
        BYTE_BUFFER => ClassKind::ByteBuffer,
        NULL => ClassKind::Null,
        DYNAMIC => ClassKind::Dynamic,
        VOID => ClassKind::Void,
        NEVER => ClassKind::Never,
        //NUM_PREDEFINED_CIDS => ClassKind::NumPredefinedKinds,
        _ => ClassKind::Illegal,
    }
}
