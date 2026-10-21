use crate::{constants::ClassId, profile_reader::{
    profile::{SnapshotArchitecture, SnapshotFeatures},
    profile_json_dictionary::*,
}};
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

// read the data_profile file and return a (ops, cid map) tuple
pub(super) fn read_profile_file(
    file_name: &String,
) -> anyhow::Result<(HashMap<String, serde_json::Value>, HashMap<String, u32>)> {
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
    let class_map = serde_json::from_value::<HashMap<String, u32>>(class_map_value)?;

    Ok((ops, class_map))
}

pub fn class_name_txlate(class_name: &str) -> ClassId {
    match class_name {
        ILLEGAL => ClassId::IllegalCid,
        NATIVE_POINTER => ClassId::NativePointer,
        FREE_LIST_ELEMENT => ClassId::FreeListElement,
        FORWARDING_CORPSE => ClassId::ForwardingCorpse,
        OBJECT => ClassId::ObjectCid,
        CLASS => ClassId::ClassCid,
        PATCH_CLASS => ClassId::PatchClassCid,
        FUNCTION => ClassId::FunctionCid,
        TYPE_PARAMETERS => ClassId::TypeParametersCid,
        CLOSURE_DATA => ClassId::ClosureDataCid,
        FFI_TRAMPOLINE_DATA => ClassId::FfiTrampolineDataCid,
        FIELD => ClassId::FieldCid,
        SCRIPT => ClassId::ScriptCid,
        LIBRARY => ClassId::LibraryCid,
        NAMESPACE => ClassId::NamespaceCid,
        KERNEL_PROGRAM_INFO => ClassId::KernelProgramInfoCid,
        WEAK_SERIALIZATION_REFERENCE => ClassId::WeakSerializationReferenceCid,
        WEAK_ARRAY => ClassId::WeakArrayCid,
        CODE => ClassId::CodeCid,
        BYTECODE => ClassId::BytecodeCid,
        INSTRUCTIONS => ClassId::InstructionsCid,
        INSTRUCTIONS_SECTION => ClassId::InstructionsSectionCid,
        INSTRUCTIONS_TABLE => ClassId::InstructionsTableCid,
        OBJECT_POOL => ClassId::ObjectPoolCid,
        PC_DESCRIPTORS => ClassId::PcDescriptorsCid,
        CODE_SOURCE_MAP => ClassId::CodeSourceMapCid,
        COMPRESSED_STACK_MAPS => ClassId::CompressedStackMapsCid,
        LOCAL_VAR_DESCRIPTORS => ClassId::LocalVarDescriptorsCid,
        EXCEPTION_HANDLERS => ClassId::ExceptionHandlersCid,
        CONTEXT => ClassId::ContextCid,
        CONTEXT_SCOPE => ClassId::ContextScopeCid,
        SENTINEL => ClassId::SentinelCid,
        SINGLE_TARGET_CACHE => ClassId::SingleTargetCacheCid,
        MONOMORPHIC_SMIABLE_CALL => ClassId::MonomorphicSmiableCallCid,
        CALL_SITE_DATA => ClassId::CallSiteDataCid,
        UNLINKED_CALL => ClassId::UnlinkedCallCid,
        IC_DATA => ClassId::ICDataCid,
        MEGAMORPHIC_CACHE => ClassId::MegamorphicCacheCid,
        SUBTYPE_TEST_CACHE => ClassId::SubtypeTestCacheCid,
        LOADING_UNIT => ClassId::LoadingUnitCid,
        ERROR => ClassId::ErrorCid,
        API_ERROR => ClassId::ApiErrorCid,
        LANGUAGE_ERROR => ClassId::LanguageErrorCid,
        UNHANDLED_EXCEPTION => ClassId::UnhandledExceptionCid,
        UNWIND_ERROR => ClassId::UnwindErrorCid,
        INSTANCE => ClassId::InstanceCid,
        LIBRARY_PREFIX => ClassId::LibraryPrefixCid,
        TYPE_ARGUMENTS => ClassId::TypeArgumentsCid,
        ABSTRACT_TYPE => ClassId::AbstractTypeCid,
        TYPE => ClassId::TypeCid,
        FUNCTION_TYPE => ClassId::FunctionTypeCid,
        RECORD_TYPE => ClassId::RecordTypeCid,
        TYPE_PARAMETER => ClassId::TypeParameterCid,
        FINALIZER_BASE => ClassId::FinalizerBaseCid,
        FINALIZER => ClassId::FinalizerCid,
        NATIVE_FINALIZER => ClassId::NativeFinalizerCid,
        FINALIZER_ENTRY => ClassId::FinalizerEntryCid,
        CLOSURE => ClassId::ClosureCid,
        NUMBER => ClassId::NumberCid,
        INTEGER => ClassId::IntegerCid,
        SMI => ClassId::SmiCid,
        MINT => ClassId::MintCid,
        DOUBLE => ClassId::DoubleCid,
        BOOL => ClassId::BoolCid,
        FLOAT32X4 => ClassId::Float32x4Cid,
        INT32X4 => ClassId::Int32x4Cid,
        FLOAT64X2 => ClassId::Float64x2Cid,
        RECORD => ClassId::RecordCid,
        TYPED_DATA_BASE => ClassId::TypedDataBaseCid,
        TYPED_DATA => ClassId::TypedDataCid,
        EXTERNAL_TYPED_DATA => ClassId::ExternalTypedDataCid,
        TYPED_DATA_VIEW => ClassId::TypedDataViewCid,
        POINTER => ClassId::PointerCid,
        DYNAMIC_LIBRARY => ClassId::DynamicLibraryCid,
        CAPABILITY => ClassId::CapabilityCid,
        RECEIVE_PORT => ClassId::ReceivePortCid,
        SEND_PORT => ClassId::SendPortCid,
        STACK_TRACE => ClassId::StackTraceCid,
        SUSPEND_STATE => ClassId::SuspendStateCid,
        REG_EXP => ClassId::RegExpCid,
        WEAK_PROPERTY => ClassId::WeakPropertyCid,
        WEAK_REFERENCE => ClassId::WeakReferenceCid,
        MIRROR_REFERENCE => ClassId::MirrorReferenceCid,
        FUTURE_OR => ClassId::FutureOrCid,
        USER_TAG => ClassId::UserTagCid,
        TRANSFERABLE_TYPED_DATA => ClassId::TransferableTypedDataCid,
        MAP => ClassId::MapCid,
        CONST_MAP => ClassId::ConstMapCid,
        SET => ClassId::SetCid,
        CONST_SET => ClassId::ConstSetCid,
        ARRAY => ClassId::ArrayCid,
        IMMUTABLE_ARRAY => ClassId::ImmutableArrayCid,
        GROWABLE_OBJECT_ARRAY => ClassId::GrowableObjectArrayCid,
        STRING => ClassId::_StringCid,
        ONE_BYTE_STRING => ClassId::OneByteStringCid,
        TWO_BYTE_STRING => ClassId::TwoByteStringCid,
        FFI_NATIVE_FUNCTION => ClassId::FfiNativeFunctionCid,
        FFI_INT8 => ClassId::FfiInt8Cid,
        FFI_INT16 => ClassId::FfiInt16Cid,
        FFI_INT32 => ClassId::FfiInt32Cid,
        FFI_INT64 => ClassId::FfiInt64Cid,
        FFI_UINT8 => ClassId::FfiUint8Cid,
        FFI_UINT16 => ClassId::FfiUint16Cid,
        FFI_UINT32 => ClassId::FfiUint32Cid,
        FFI_UINT64 => ClassId::FfiUint64Cid,
        FFI_FLOAT => ClassId::FfiFloatCid,
        FFI_DOUBLE => ClassId::FfiDoubleCid,
        FFI_VOID => ClassId::FfiVoidCid,
        FFI_HANDLE => ClassId::FfiHandleCid,
        FFI_BOOL => ClassId::FfiBoolCid,
        FFI_NATIVE_TYPE => ClassId::FfiNativeTypeCid,
        FFI_STRUCT => ClassId::FfiStructCid,
        TYPED_DATA_INT8_ARRAY => ClassId::TypedDataInt8ArrayCid,
        TYPED_DATA_INT8_ARRAY_VIEW => ClassId::TypedDataInt8ArrayViewCid,
        EXTERNAL_TYPED_DATA_INT8_ARRAY => ClassId::ExternalTypedDataInt8ArrayCid,
        UNMODIFIABLE_TYPED_DATA_INT8_ARRAY_VIEW => ClassId::UnmodifiableTypedDataInt8ArrayViewCid,
        TYPED_DATA_UINT8_ARRAY => ClassId::TypedDataUint8ArrayCid,
        TYPED_DATA_UINT8_ARRAY_VIEW => ClassId::TypedDataUint8ArrayViewCid,
        EXTERNAL_TYPED_DATA_UINT8_ARRAY => ClassId::ExternalTypedDataUint8ArrayCid,
        UNMODIFIABLE_TYPED_DATA_UINT8_ARRAY_VIEW => ClassId::UnmodifiableTypedDataUint8ArrayViewCid,
        TYPED_DATA_UINT8_CLAMPED_ARRAY => ClassId::TypedDataUint8ClampedArrayCid,
        TYPED_DATA_UINT8_CLAMPED_ARRAY_VIEW => ClassId::TypedDataUint8ClampedArrayViewCid,
        EXTERNAL_TYPED_DATA_UINT8_CLAMPED_ARRAY => ClassId::ExternalTypedDataUint8ClampedArrayCid,
        UNMODIFIABLE_TYPED_DATA_UINT8_CLAMPED_ARRAY_VIEW => ClassId::UnmodifiableTypedDataUint8ClampedArrayViewCid,
        TYPED_DATA_INT16_ARRAY => ClassId::TypedDataInt16ArrayCid,
        TYPED_DATA_INT16_ARRAY_VIEW => ClassId::TypedDataInt16ArrayViewCid,
        EXTERNAL_TYPED_DATA_INT16_ARRAY => ClassId::ExternalTypedDataInt16ArrayCid,
        UNMODIFIABLE_TYPED_DATA_INT16_ARRAY_VIEW => ClassId::UnmodifiableTypedDataInt16ArrayViewCid,
        TYPED_DATA_UINT16_ARRAY => ClassId::TypedDataUint16ArrayCid,
        TYPED_DATA_UINT16_ARRAY_VIEW => ClassId::TypedDataUint16ArrayViewCid,
        EXTERNAL_TYPED_DATA_UINT16_ARRAY => ClassId::ExternalTypedDataUint16ArrayCid,
        UNMODIFIABLE_TYPED_DATA_UINT16_ARRAY_VIEW => ClassId::UnmodifiableTypedDataUint16ArrayViewCid,
        TYPED_DATA_INT32_ARRAY => ClassId::TypedDataInt32ArrayCid,
        TYPED_DATA_INT32_ARRAY_VIEW => ClassId::TypedDataInt32ArrayViewCid,
        EXTERNAL_TYPED_DATA_INT32_ARRAY => ClassId::ExternalTypedDataInt32ArrayCid,
        UNMODIFIABLE_TYPED_DATA_INT32_ARRAY_VIEW => ClassId::UnmodifiableTypedDataInt32ArrayViewCid,
        TYPED_DATA_UINT32_ARRAY => ClassId::TypedDataUint32ArrayCid,
        TYPED_DATA_UINT32_ARRAY_VIEW => ClassId::TypedDataUint32ArrayViewCid,
        EXTERNAL_TYPED_DATA_UINT32_ARRAY => ClassId::ExternalTypedDataUint32ArrayCid,
        UNMODIFIABLE_TYPED_DATA_UINT32_ARRAY_VIEW => ClassId::UnmodifiableTypedDataUint32ArrayViewCid,
        TYPED_DATA_INT64_ARRAY => ClassId::TypedDataInt64ArrayCid,
        TYPED_DATA_INT64_ARRAY_VIEW => ClassId::TypedDataInt64ArrayViewCid,
        EXTERNAL_TYPED_DATA_INT64_ARRAY => ClassId::ExternalTypedDataInt64ArrayCid,
        UNMODIFIABLE_TYPED_DATA_INT64_ARRAY_VIEW => ClassId::UnmodifiableTypedDataInt64ArrayViewCid,
        TYPED_DATA_UINT64_ARRAY => ClassId::TypedDataUint64ArrayCid,
        TYPED_DATA_UINT64_ARRAY_VIEW => ClassId::TypedDataUint64ArrayViewCid,
        EXTERNAL_TYPED_DATA_UINT64_ARRAY => ClassId::ExternalTypedDataUint64ArrayCid,
        UNMODIFIABLE_TYPED_DATA_UINT64_ARRAY_VIEW => ClassId::UnmodifiableTypedDataUint64ArrayViewCid,
        TYPED_DATA_FLOAT32_ARRAY => ClassId::TypedDataFloat32ArrayCid,
        TYPED_DATA_FLOAT32_ARRAY_VIEW => ClassId::TypedDataFloat32ArrayViewCid,
        EXTERNAL_TYPED_DATA_FLOAT32_ARRAY => ClassId::ExternalTypedDataFloat32ArrayCid,
        UNMODIFIABLE_TYPED_DATA_FLOAT32_ARRAY_VIEW => ClassId::UnmodifiableTypedDataFloat32ArrayViewCid,
        TYPED_DATA_FLOAT64_ARRAY => ClassId::TypedDataFloat64ArrayCid,
        TYPED_DATA_FLOAT64_ARRAY_VIEW => ClassId::TypedDataFloat64ArrayViewCid,
        EXTERNAL_TYPED_DATA_FLOAT64_ARRAY => ClassId::ExternalTypedDataFloat64ArrayCid,
        UNMODIFIABLE_TYPED_DATA_FLOAT64_ARRAY_VIEW => ClassId::UnmodifiableTypedDataFloat64ArrayViewCid,
        TYPED_DATA_FLOAT32X4_ARRAY => ClassId::TypedDataFloat32x4ArrayCid,
        TYPED_DATA_FLOAT32X4_ARRAY_VIEW => ClassId::TypedDataFloat32x4ArrayViewCid,
        EXTERNAL_TYPED_DATA_FLOAT32X4_ARRAY => ClassId::ExternalTypedDataFloat32x4ArrayCid,
        UNMODIFIABLE_TYPED_DATA_FLOAT32X4_ARRAY_VIEW => ClassId::UnmodifiableTypedDataFloat32x4ArrayViewCid,
        TYPED_DATA_INT32X4_ARRAY => ClassId::TypedDataInt32x4ArrayCid,
        TYPED_DATA_INT32X4_ARRAY_VIEW => ClassId::TypedDataInt32x4ArrayViewCid,
        EXTERNAL_TYPED_DATA_INT32X4_ARRAY => ClassId::ExternalTypedDataInt32x4ArrayCid,
        UNMODIFIABLE_TYPED_DATA_INT32X4_ARRAY_VIEW => ClassId::UnmodifiableTypedDataInt32x4ArrayViewCid,
        TYPED_DATA_FLOAT64X2_ARRAY => ClassId::TypedDataFloat64x2ArrayCid,
        TYPED_DATA_FLOAT64X2_ARRAY_VIEW => ClassId::TypedDataFloat64x2ArrayViewCid,
        EXTERNAL_TYPED_DATA_FLOAT64X2_ARRAY => ClassId::ExternalTypedDataFloat64x2ArrayCid,
        UNMODIFIABLE_TYPED_DATA_FLOAT64X2_ARRAY_VIEW => ClassId::UnmodifiableTypedDataFloat64x2ArrayViewCid,
        BYTE_DATA_VIEW => ClassId::ByteDataViewCid,
        UNMODIFIABLE_BYTE_DATA_VIEW => ClassId::UnmodifiableByteDataViewCid,
        BYTE_BUFFER => ClassId::ByteBufferCid,
        NULL => ClassId::NullCid,
        DYNAMIC => ClassId::DynamicCid,
        VOID => ClassId::VoidCid,
        NEVER => ClassId::NeverCid,
        NUM_PREDEFINED_CIDS => ClassId::NumPredefinedCids,
        _ => ClassId::IllegalCid,
    }
}
