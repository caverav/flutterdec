use crate::{
    constants::ClassKind,
    profile_reader::{
        profile::{SnapshotArchitecture, SnapshotFeatures},
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
)> {
    let profile_file: Vec<u8>;

    if let Ok(profiles_dirpath) = env::var(PROFILE_PATH_ENV_VAR_NAME) {
        let full_path = profiles_dirpath + "/" + file_name;
        profile_file =
            fs::read(&full_path).with_context(|| format!("read data profile: {}", full_path))?;
    } else {
        // fallback #1 try reading the working directory
        profile_file = fs::read(format!("./{}", file_name)).with_context(|| {
            format!(
                "read data profile from current working directory: {}",
                file_name
            )
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

    let mut cid_map = HashMap::with_capacity(class_names.len());
    let mut class_kind_to_cid = HashMap::with_capacity(class_names.len());
    for (raw_cid, class_name) in class_names {
        let class_kind = class_name_txlate(&class_name);
        anyhow::ensure!(
            class_kind != ClassKind::IllegalCid || class_name == ILLEGAL,
            "unknown class name {class_name:?} for CID {raw_cid} in profile"
        );
        if let Some(previous_cid) = class_kind_to_cid.insert(class_kind, raw_cid) {
            anyhow::bail!(
                "profile maps {class_name:?} to both CID {previous_cid} and CID {raw_cid}"
            );
        }
        cid_map.insert(raw_cid, class_kind);
    }

    Ok((ops, cid_map, num_predefined_cids, class_kind_to_cid))
}

// Strictly speaking, this function is not required. We could change decide_cluster to use the &str constants
// defined in profile_json_dictioanry.rs, but this seemingly pointless mapping exists to give the profile data
// transparency from the other components viewpoints, this is, the info_producer or other operations that operate on
// object data obtained during cluster deserialization do NOT need to know specifics or any detail whatsoever defined
// in the data profile. Instead, those details are abstracted away via mapping them to stable constants or states. See the
// comment for the DEFINE_CLASS_KIND! macro in src/constants.rs to know more.
pub fn class_name_txlate(class_name: &str) -> ClassKind {
    match class_name {
        ILLEGAL => ClassKind::IllegalCid,
        NATIVE_POINTER => ClassKind::NativePointer,
        FREE_LIST_ELEMENT => ClassKind::FreeListElement,
        FORWARDING_CORPSE => ClassKind::ForwardingCorpse,
        OBJECT => ClassKind::ObjectCid,
        CLASS => ClassKind::ClassCid,
        PATCH_CLASS => ClassKind::PatchClassCid,
        FUNCTION => ClassKind::FunctionCid,
        TYPE_PARAMETERS => ClassKind::TypeParametersCid,
        CLOSURE_DATA => ClassKind::ClosureDataCid,
        FFI_TRAMPOLINE_DATA => ClassKind::FfiTrampolineDataCid,
        FIELD => ClassKind::FieldCid,
        SCRIPT => ClassKind::ScriptCid,
        LIBRARY => ClassKind::LibraryCid,
        NAMESPACE => ClassKind::NamespaceCid,
        KERNEL_PROGRAM_INFO => ClassKind::KernelProgramInfoCid,
        WEAK_SERIALIZATION_REFERENCE => ClassKind::WeakSerializationReferenceCid,
        WEAK_ARRAY => ClassKind::WeakArrayCid,
        CODE => ClassKind::CodeCid,
        BYTECODE => ClassKind::BytecodeCid,
        INSTRUCTIONS => ClassKind::InstructionsCid,
        INSTRUCTIONS_SECTION => ClassKind::InstructionsSectionCid,
        INSTRUCTIONS_TABLE => ClassKind::InstructionsTableCid,
        OBJECT_POOL => ClassKind::ObjectPoolCid,
        PC_DESCRIPTORS => ClassKind::PcDescriptorsCid,
        CODE_SOURCE_MAP => ClassKind::CodeSourceMapCid,
        COMPRESSED_STACK_MAPS => ClassKind::CompressedStackMapsCid,
        LOCAL_VAR_DESCRIPTORS => ClassKind::LocalVarDescriptorsCid,
        EXCEPTION_HANDLERS => ClassKind::ExceptionHandlersCid,
        CONTEXT => ClassKind::ContextCid,
        CONTEXT_SCOPE => ClassKind::ContextScopeCid,
        SENTINEL => ClassKind::SentinelCid,
        SINGLE_TARGET_CACHE => ClassKind::SingleTargetCacheCid,
        MONOMORPHIC_SMIABLE_CALL => ClassKind::MonomorphicSmiableCallCid,
        CALL_SITE_DATA => ClassKind::CallSiteDataCid,
        UNLINKED_CALL => ClassKind::UnlinkedCallCid,
        IC_DATA => ClassKind::ICDataCid,
        MEGAMORPHIC_CACHE => ClassKind::MegamorphicCacheCid,
        SUBTYPE_TEST_CACHE => ClassKind::SubtypeTestCacheCid,
        LOADING_UNIT => ClassKind::LoadingUnitCid,
        ERROR => ClassKind::ErrorCid,
        API_ERROR => ClassKind::ApiErrorCid,
        LANGUAGE_ERROR => ClassKind::LanguageErrorCid,
        UNHANDLED_EXCEPTION => ClassKind::UnhandledExceptionCid,
        UNWIND_ERROR => ClassKind::UnwindErrorCid,
        INSTANCE => ClassKind::InstanceCid,
        LIBRARY_PREFIX => ClassKind::LibraryPrefixCid,
        TYPE_ARGUMENTS => ClassKind::TypeArgumentsCid,
        ABSTRACT_TYPE => ClassKind::AbstractTypeCid,
        TYPE => ClassKind::TypeCid,
        FUNCTION_TYPE => ClassKind::FunctionTypeCid,
        RECORD_TYPE => ClassKind::RecordTypeCid,
        TYPE_PARAMETER => ClassKind::TypeParameterCid,
        FINALIZER_BASE => ClassKind::FinalizerBaseCid,
        FINALIZER => ClassKind::FinalizerCid,
        NATIVE_FINALIZER => ClassKind::NativeFinalizerCid,
        FINALIZER_ENTRY => ClassKind::FinalizerEntryCid,
        CLOSURE => ClassKind::ClosureCid,
        NUMBER => ClassKind::NumberCid,
        INTEGER => ClassKind::IntegerCid,
        SMI => ClassKind::SmiCid,
        MINT => ClassKind::MintCid,
        DOUBLE => ClassKind::DoubleCid,
        BOOL => ClassKind::BoolCid,
        FLOAT32X4 => ClassKind::Float32x4Cid,
        INT32X4 => ClassKind::Int32x4Cid,
        FLOAT64X2 => ClassKind::Float64x2Cid,
        RECORD => ClassKind::RecordCid,
        TYPED_DATA_BASE => ClassKind::TypedDataBaseCid,
        TYPED_DATA => ClassKind::TypedDataCid,
        EXTERNAL_TYPED_DATA => ClassKind::ExternalTypedDataCid,
        TYPED_DATA_VIEW => ClassKind::TypedDataViewCid,
        POINTER => ClassKind::PointerCid,
        DYNAMIC_LIBRARY => ClassKind::DynamicLibraryCid,
        CAPABILITY => ClassKind::CapabilityCid,
        RECEIVE_PORT => ClassKind::ReceivePortCid,
        SEND_PORT => ClassKind::SendPortCid,
        STACK_TRACE => ClassKind::StackTraceCid,
        SUSPEND_STATE => ClassKind::SuspendStateCid,
        REG_EXP => ClassKind::RegExpCid,
        WEAK_PROPERTY => ClassKind::WeakPropertyCid,
        WEAK_REFERENCE => ClassKind::WeakReferenceCid,
        MIRROR_REFERENCE => ClassKind::MirrorReferenceCid,
        FUTURE_OR => ClassKind::FutureOrCid,
        USER_TAG => ClassKind::UserTagCid,
        TRANSFERABLE_TYPED_DATA => ClassKind::TransferableTypedDataCid,
        MAP => ClassKind::MapCid,
        CONST_MAP => ClassKind::ConstMapCid,
        SET => ClassKind::SetCid,
        CONST_SET => ClassKind::ConstSetCid,
        ARRAY => ClassKind::ArrayCid,
        IMMUTABLE_ARRAY => ClassKind::ImmutableArrayCid,
        GROWABLE_OBJECT_ARRAY => ClassKind::GrowableObjectArrayCid,
        STRING => ClassKind::_StringCid,
        ONE_BYTE_STRING => ClassKind::OneByteStringCid,
        TWO_BYTE_STRING => ClassKind::TwoByteStringCid,
        FFI_NATIVE_FUNCTION => ClassKind::FfiNativeFunctionCid,
        FFI_INT8 => ClassKind::FfiInt8Cid,
        FFI_INT16 => ClassKind::FfiInt16Cid,
        FFI_INT32 => ClassKind::FfiInt32Cid,
        FFI_INT64 => ClassKind::FfiInt64Cid,
        FFI_UINT8 => ClassKind::FfiUint8Cid,
        FFI_UINT16 => ClassKind::FfiUint16Cid,
        FFI_UINT32 => ClassKind::FfiUint32Cid,
        FFI_UINT64 => ClassKind::FfiUint64Cid,
        FFI_FLOAT => ClassKind::FfiFloatCid,
        FFI_DOUBLE => ClassKind::FfiDoubleCid,
        FFI_VOID => ClassKind::FfiVoidCid,
        FFI_HANDLE => ClassKind::FfiHandleCid,
        FFI_BOOL => ClassKind::FfiBoolCid,
        FFI_NATIVE_TYPE => ClassKind::FfiNativeTypeCid,
        FFI_STRUCT => ClassKind::FfiStructCid,
        TYPED_DATA_INT8_ARRAY => ClassKind::TypedDataInt8ArrayCid,
        TYPED_DATA_INT8_ARRAY_VIEW => ClassKind::TypedDataInt8ArrayViewCid,
        EXTERNAL_TYPED_DATA_INT8_ARRAY => ClassKind::ExternalTypedDataInt8ArrayCid,
        UNMODIFIABLE_TYPED_DATA_INT8_ARRAY_VIEW => ClassKind::UnmodifiableTypedDataInt8ArrayViewCid,
        TYPED_DATA_UINT8_ARRAY => ClassKind::TypedDataUint8ArrayCid,
        TYPED_DATA_UINT8_ARRAY_VIEW => ClassKind::TypedDataUint8ArrayViewCid,
        EXTERNAL_TYPED_DATA_UINT8_ARRAY => ClassKind::ExternalTypedDataUint8ArrayCid,
        UNMODIFIABLE_TYPED_DATA_UINT8_ARRAY_VIEW => ClassKind::UnmodifiableTypedDataUint8ArrayViewCid,
        TYPED_DATA_UINT8_CLAMPED_ARRAY => ClassKind::TypedDataUint8ClampedArrayCid,
        TYPED_DATA_UINT8_CLAMPED_ARRAY_VIEW => ClassKind::TypedDataUint8ClampedArrayViewCid,
        EXTERNAL_TYPED_DATA_UINT8_CLAMPED_ARRAY => ClassKind::ExternalTypedDataUint8ClampedArrayCid,
        UNMODIFIABLE_TYPED_DATA_UINT8_CLAMPED_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataUint8ClampedArrayViewCid
        }
        TYPED_DATA_INT16_ARRAY => ClassKind::TypedDataInt16ArrayCid,
        TYPED_DATA_INT16_ARRAY_VIEW => ClassKind::TypedDataInt16ArrayViewCid,
        EXTERNAL_TYPED_DATA_INT16_ARRAY => ClassKind::ExternalTypedDataInt16ArrayCid,
        UNMODIFIABLE_TYPED_DATA_INT16_ARRAY_VIEW => ClassKind::UnmodifiableTypedDataInt16ArrayViewCid,
        TYPED_DATA_UINT16_ARRAY => ClassKind::TypedDataUint16ArrayCid,
        TYPED_DATA_UINT16_ARRAY_VIEW => ClassKind::TypedDataUint16ArrayViewCid,
        EXTERNAL_TYPED_DATA_UINT16_ARRAY => ClassKind::ExternalTypedDataUint16ArrayCid,
        UNMODIFIABLE_TYPED_DATA_UINT16_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataUint16ArrayViewCid
        }
        TYPED_DATA_INT32_ARRAY => ClassKind::TypedDataInt32ArrayCid,
        TYPED_DATA_INT32_ARRAY_VIEW => ClassKind::TypedDataInt32ArrayViewCid,
        EXTERNAL_TYPED_DATA_INT32_ARRAY => ClassKind::ExternalTypedDataInt32ArrayCid,
        UNMODIFIABLE_TYPED_DATA_INT32_ARRAY_VIEW => ClassKind::UnmodifiableTypedDataInt32ArrayViewCid,
        TYPED_DATA_UINT32_ARRAY => ClassKind::TypedDataUint32ArrayCid,
        TYPED_DATA_UINT32_ARRAY_VIEW => ClassKind::TypedDataUint32ArrayViewCid,
        EXTERNAL_TYPED_DATA_UINT32_ARRAY => ClassKind::ExternalTypedDataUint32ArrayCid,
        UNMODIFIABLE_TYPED_DATA_UINT32_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataUint32ArrayViewCid
        }
        TYPED_DATA_INT64_ARRAY => ClassKind::TypedDataInt64ArrayCid,
        TYPED_DATA_INT64_ARRAY_VIEW => ClassKind::TypedDataInt64ArrayViewCid,
        EXTERNAL_TYPED_DATA_INT64_ARRAY => ClassKind::ExternalTypedDataInt64ArrayCid,
        UNMODIFIABLE_TYPED_DATA_INT64_ARRAY_VIEW => ClassKind::UnmodifiableTypedDataInt64ArrayViewCid,
        TYPED_DATA_UINT64_ARRAY => ClassKind::TypedDataUint64ArrayCid,
        TYPED_DATA_UINT64_ARRAY_VIEW => ClassKind::TypedDataUint64ArrayViewCid,
        EXTERNAL_TYPED_DATA_UINT64_ARRAY => ClassKind::ExternalTypedDataUint64ArrayCid,
        UNMODIFIABLE_TYPED_DATA_UINT64_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataUint64ArrayViewCid
        }
        TYPED_DATA_FLOAT32_ARRAY => ClassKind::TypedDataFloat32ArrayCid,
        TYPED_DATA_FLOAT32_ARRAY_VIEW => ClassKind::TypedDataFloat32ArrayViewCid,
        EXTERNAL_TYPED_DATA_FLOAT32_ARRAY => ClassKind::ExternalTypedDataFloat32ArrayCid,
        UNMODIFIABLE_TYPED_DATA_FLOAT32_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataFloat32ArrayViewCid
        }
        TYPED_DATA_FLOAT64_ARRAY => ClassKind::TypedDataFloat64ArrayCid,
        TYPED_DATA_FLOAT64_ARRAY_VIEW => ClassKind::TypedDataFloat64ArrayViewCid,
        EXTERNAL_TYPED_DATA_FLOAT64_ARRAY => ClassKind::ExternalTypedDataFloat64ArrayCid,
        UNMODIFIABLE_TYPED_DATA_FLOAT64_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataFloat64ArrayViewCid
        }
        TYPED_DATA_FLOAT32X4_ARRAY => ClassKind::TypedDataFloat32x4ArrayCid,
        TYPED_DATA_FLOAT32X4_ARRAY_VIEW => ClassKind::TypedDataFloat32x4ArrayViewCid,
        EXTERNAL_TYPED_DATA_FLOAT32X4_ARRAY => ClassKind::ExternalTypedDataFloat32x4ArrayCid,
        UNMODIFIABLE_TYPED_DATA_FLOAT32X4_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataFloat32x4ArrayViewCid
        }
        TYPED_DATA_INT32X4_ARRAY => ClassKind::TypedDataInt32x4ArrayCid,
        TYPED_DATA_INT32X4_ARRAY_VIEW => ClassKind::TypedDataInt32x4ArrayViewCid,
        EXTERNAL_TYPED_DATA_INT32X4_ARRAY => ClassKind::ExternalTypedDataInt32x4ArrayCid,
        UNMODIFIABLE_TYPED_DATA_INT32X4_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataInt32x4ArrayViewCid
        }
        TYPED_DATA_FLOAT64X2_ARRAY => ClassKind::TypedDataFloat64x2ArrayCid,
        TYPED_DATA_FLOAT64X2_ARRAY_VIEW => ClassKind::TypedDataFloat64x2ArrayViewCid,
        EXTERNAL_TYPED_DATA_FLOAT64X2_ARRAY => ClassKind::ExternalTypedDataFloat64x2ArrayCid,
        UNMODIFIABLE_TYPED_DATA_FLOAT64X2_ARRAY_VIEW => {
            ClassKind::UnmodifiableTypedDataFloat64x2ArrayViewCid
        }
        BYTE_DATA_VIEW => ClassKind::ByteDataViewCid,
        UNMODIFIABLE_BYTE_DATA_VIEW => ClassKind::UnmodifiableByteDataViewCid,
        BYTE_BUFFER => ClassKind::ByteBufferCid,
        NULL => ClassKind::NullCid,
        DYNAMIC => ClassKind::DynamicCid,
        VOID => ClassKind::VoidCid,
        NEVER => ClassKind::NeverCid,
        NUM_PREDEFINED_CIDS => ClassKind::NumPredefinedCids,
        _ => ClassKind::IllegalCid,
    }
}
