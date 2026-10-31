#![allow(dead_code)]

/*
    We will use the enums here to define a predetermined number of fields for the
    native types. Some of these fields are invariants for a certain DTO,
    this is, even if some of the field's order (offset) or size change, we agree
    that they will always be present. For example, a Function object "name" field will always
    exist regardless of where in the class' layout it is at. Through the introduction of these enums,
    we can use the invariants to keep the object definitions in mod.rs as DTOs instead of strict contracts.

    These fields are not all invariants, though. Some fields are optional and probably will not even be read
    depending on the distribution flag for the final binary (i.e product vs debug), so the enums here serve as
    a STABLE vocabulary whose aim is to cover all of the different version-dependent schemas. Thus, the profiles
    will always conform to this vocabulary, ultimately there being most of the time
    a 1-to-1 map between their semantic types and the fields in the enums below.

    The info_producer module and other pieces of code that make use of object fields such as resolve_entrypoints
    do NOT need to know how the clustered data and the DTOs came to be populated, only that
    they exist. This will help maintain that transparency of the deserialization process from the point of view
    of the program model generation code or code that mimics post-load behavior.
*/

use std::any::Any;
use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::profile_reader::profile_json_dictionary;
use crate::raw_object::PropType;

pub(crate) trait CoreObject {
    type Field: DeserializeOwned + Copy + std::fmt::Debug;

    const PROFILE_NAME: &'static str;

    fn set_core_field(&mut self, field: Self::Field, value: PropType) -> Result<()>;
    fn core_field(&self, field: Self::Field) -> &dyn Any;
    fn core_field_mut(&mut self, field: Self::Field) -> &mut dyn Any;
    fn extra_properties_mut(&mut self) -> &mut HashMap<String, PropType>;
}

trait FromPropType: Sized {
    fn from_prop_type(value: PropType) -> Result<Self>;
}

fn integer_value(value: PropType) -> Result<i128> {
    match value {
        PropType::Byte(value) => Ok(i128::from(value)),
        PropType::RefId(value) | PropType::RawU32(value) => Ok(i128::from(value)),
        PropType::Signed(value) => Ok(i128::from(value)),
        PropType::Unsigned(value) | PropType::RawU64(value) => Ok(i128::from(value)),
        PropType::Bytes(_) => bail!("a byte payload cannot be assigned to an integer"),
        PropType::Array(_) => bail!("an array cannot be assigned to an integer"),
    }
}

macro_rules! IMPLEMENT_SIGNED_PROPERTY {
    ($( $type:ty ),* $(,)?) => {
        $(
            impl FromPropType for $type {
                fn from_prop_type(value: PropType) -> Result<Self> {
                    let value = integer_value(value)?;
                    Self::try_from(value).map_err(|_| {
                        anyhow!("value {value} is outside the range of {}", stringify!($type))
                    })
                }
            }
        )*
    };
}

macro_rules! IMPLEMENT_UNSIGNED_PROPERTY {
    ($( $type:ty ),* $(,)?) => {
        $(
            impl FromPropType for $type {
                fn from_prop_type(value: PropType) -> Result<Self> {
                    if let PropType::Signed(value) = &value {
                        if *value < 0 {
                            let signed_min = -(1_i128 << (Self::BITS - 1));
                            if i128::from(*value) < signed_min {
                                bail!(
                                    "value {value} is outside the signed encoding range of {}",
                                    stringify!($type)
                                );
                            }
                            return Ok(*value as Self);
                        }
                    }
                    let value = integer_value(value)?;
                    Self::try_from(value).map_err(|_| {
                        anyhow!("value {value} is outside the range of {}", stringify!($type))
                    })
                }
            }
        )*
    };
}

IMPLEMENT_SIGNED_PROPERTY!(i16, i32, i64, isize);
IMPLEMENT_UNSIGNED_PROPERTY!(u8, u16, u32, u64, usize);

impl FromPropType for i8 {
    fn from_prop_type(value: PropType) -> Result<Self> {
        if let PropType::Byte(value) = value {
            return Ok(value as i8);
        }
        let value = integer_value(value)?;
        Self::try_from(value).map_err(|_| anyhow!("value {value} is outside the range of i8"))
    }
}

impl FromPropType for bool {
    fn from_prop_type(value: PropType) -> Result<Self> {
        Ok(integer_value(value)? != 0)
    }
}

impl FromPropType for f64 {
    fn from_prop_type(value: PropType) -> Result<Self> {
        match value {
            PropType::RawU64(value) => Ok(Self::from_bits(value)),
            PropType::Signed(value) => Ok(Self::from_bits(value as u64)),
            _ => bail!("an f64 field requires a signed or raw_u64 bit pattern"),
        }
    }
}

impl FromPropType for Vec<u8> {
    fn from_prop_type(value: PropType) -> Result<Self> {
        match value {
            PropType::Bytes(value) => Ok(value),
            _ => bail!("a byte vector field requires bytes"),
        }
    }
}

impl FromPropType for Option<u64> {
    fn from_prop_type(value: PropType) -> Result<Self> {
        Ok(Some(u64::from_prop_type(value)?))
    }
}

macro_rules! IMPLEMENT_COMPOSITE_PROPERTY {
    ($( $type:ty ),* $(,)?) => {
        $(
            impl FromPropType for $type {
                fn from_prop_type(_value: PropType) -> Result<Self> {
                    bail!(
                        "{} requires its collection or custom handler",
                        std::any::type_name::<Self>()
                    )
                }
            }
        )*
    };
}

IMPLEMENT_COMPOSITE_PROPERTY!(
    String,
    crate::raw_object::StrType,
    crate::raw_object::ObjectPoolEntryValue,
    Vec<u32>,
    Vec<crate::raw_object::ExceptionHandlerInfo>,
    Vec<crate::raw_object::ObjectPoolEntry>,
    Vec<crate::raw_object::InstanceField>,
);

macro_rules! DEFINE_CORE_FIELDS {
    (
        $object:ident, $field_enum:ident, $profile_name:ident {
            $( $variant:ident => $field:ident ),* $(,)?
        }
    ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $field_enum {
            $( $variant, )*
        }

        impl CoreObject for crate::raw_object::$object {
            type Field = $field_enum;

            const PROFILE_NAME: &'static str = profile_json_dictionary::$profile_name;

            fn set_core_field(&mut self, field: Self::Field, value: PropType) -> Result<()> {
                match field {
                    $(
                        $field_enum::$variant => {
                            self.$field = FromPropType::from_prop_type(value).map_err(
                                |error| anyhow!(
                                    "cannot assign {}.{:?}: {error}",
                                    stringify!($object),
                                    field
                                )
                            )?;
                        }
                    )*
                }
                Ok(())
            }

            fn core_field(&self, field: Self::Field) -> &dyn Any {
                match field {
                    $( $field_enum::$variant => &self.$field, )*
                }
            }

            fn core_field_mut(&mut self, field: Self::Field) -> &mut dyn Any {
                match field {
                    $( $field_enum::$variant => &mut self.$field, )*
                }
            }

            fn extra_properties_mut(&mut self) -> &mut HashMap<String, PropType> {
                &mut self.extra_properties
            }
        }
    };
}

DEFINE_CORE_FIELDS!(
    Object, ObjectField, OBJECT {
        Tags => tags,
    }
);

DEFINE_CORE_FIELDS!(
    Class, ClassField, CLASS {
        Id => id,
        IsPredefined => is_predefined,
        Name => name,
        UserName => user_name,
        Functions => functions,
        FunctionsHashTable => functions_hash_table,
        Fields => fields,
        OffsetInWordsToField => offset_in_words_to_field,
        Interfaces => interfaces,
        Script => script,
        Library => library,
        TypeParameters => type_parameters,
        SuperType => super_type,
        Constants => constants,
        DeclarationType => declaration_type,
        InvocationDispatcherCache => invocation_dispatcher_cache,
        DirectImplementors => direct_implementors,
        DirectSubclasses => direct_subclasses,
        DeclarationInstanceTypeArguments => declaration_instance_type_arguments,
        AllocationStub => allocation_stub,
        DependentCode => dependent_code,
        NumNativeFields => num_native_fields,
        StateBits => state_bits,
        KernelOffset => kernel_offset,
        NumTypeArguments => num_type_arguments,
        HostInstanceSizeInWords => host_instance_size_in_words,
        HostTypeArgumentsFieldOffsetInWords => host_type_arguments_field_offset_in_words,
        HostNextFieldOffsetInWords => host_next_field_offset_in_words,
        TargetInstanceSizeInWords => target_instance_size_in_words,
        TargetTypeArgumentsFieldOffsetInWords => target_type_arguments_field_offset_in_words,
        TargetNextFieldOffsetInWords => target_next_field_offset_in_words,
        UnboxedFieldsBitmap => unboxed_fields_bitmap,
    }
);

DEFINE_CORE_FIELDS!(
    PatchClass, PatchClassField, PATCH_CLASS {
        WrappedClass => wrapped_class,
        Script => script,
    }
);

DEFINE_CORE_FIELDS!(
    Function, FunctionField, FUNCTION {
        EntryPoint => entry_point,
        UncheckedEntryPoint => unchecked_entry_point,
        Name => name,
        Owner => owner,
        Signature => signature,
        Data => data,
        CodeIndex => code_index,
        TokenPos => token_pos,
        KindTag => kind_tag,
    }
);

DEFINE_CORE_FIELDS!(
    ClosureData, ClosureDataField, CLOSURE_DATA {
        ContextScope => context_scope,
        ParentFunction => parent_function,
        Closure => closure,
        PackedFields => packed_fields,
    }
);

DEFINE_CORE_FIELDS!(
    FfiTrampolineData, FfiTrampolineDataField, FFI_TRAMPOLINE_DATA {
        SignatureType => signature_type,
        CSignature => c_signature,
        CallbackTarget => callback_target,
        CallbackExceptionalReturn => callback_exceptional_return,
        CallbackId => callback_id,
        FfiFunctionKind => ffi_function_kind,
    }
);

DEFINE_CORE_FIELDS!(
    Field, FieldField, FIELD {
        Name => name,
        Owner => owner,
        TypeField => type_field,
        InitializerFunction => initializer_function,
        HostOffsetOrFieldId => host_offset_or_field_id,
        TokenPos => token_pos,
        EndTokenPos => end_token_pos,
        GuardedCid => guarded_cid,
        IsNullable => is_nullable,
        KindBits => kind_bits,
    }
);

DEFINE_CORE_FIELDS!(
    Script, ScriptField, SCRIPT {
        Url => url,
        KernelScriptIndex => kernel_script_index,
    }
);

DEFINE_CORE_FIELDS!(
    Library, LibraryField, LIBRARY {
        Name => name,
        Url => url,
        PrivateKey => private_key,
        Dictionary => dictionary,
        Metadata => metadata,
        ToplevelClass => toplevel_class,
        UsedScripts => used_scripts,
        LoadingUnit => loading_unit,
        Imports => imports,
        Exports => exports,
        Index => index,
        NumImports => num_imports,
        LoadState => load_state,
        Flags => flags,
    }
);

DEFINE_CORE_FIELDS!(
    Namespace, NamespaceField, NAMESPACE {
        Target => target,
        ShowNames => show_names,
        HideNames => hide_names,
        Owner => owner,
    }
);

DEFINE_CORE_FIELDS!(
    KernelProgramInfo, KernelProgramInfoField, KERNEL_PROGRAM_INFO {
        KernelComponent => kernel_component,
        StringOffsets => string_offsets,
        StringData => string_data,
        CanonicalNames => canonical_names,
        MetadataPayloads => metadata_payloads,
        MetadataMappings => metadata_mappings,
        Scripts => scripts,
        Constants => constants,
        ConstantsTable => constants_table,
        LibrariesCache => libraries_cache,
        ClassesCache => classes_cache,
    }
);

DEFINE_CORE_FIELDS!(
    CodeSourceMap, CodeSourceMapField, CODE_SOURCE_MAP {
        Length => length,
        Data => data,
    }
);

DEFINE_CORE_FIELDS!(
    CompressedStackMaps, CompressedStackMapsField, COMPRESSED_STACK_MAPS {
        Length => length,
        FlagsAndSize => flags_and_size,
        Data => data,
    }
);

DEFINE_CORE_FIELDS!(
    PcDescriptors, PcDescriptorsField, PC_DESCRIPTORS {
        Length => length,
        Data => data,
    }
);

DEFINE_CORE_FIELDS!(
    ExceptionHandlers, ExceptionHandlersField, EXCEPTION_HANDLERS {
        HandledTypesData => handled_types_data,
        PackedFields => packed_fields,
        NumEntries => num_entries,
        Entries => entries,
    }
);

DEFINE_CORE_FIELDS!(
    ExceptionHandlerInfo, ExceptionHandlerInfoField, EXCEPTION_HANDLER_INFO {
        HandlerPcOffset => handler_pc_offset,
        OuterTryIndex => outer_try_index,
        NeedsStacktrace => needs_stacktrace,
        HasCatchAll => has_catch_all,
        IsGenerated => is_generated,
    }
);

DEFINE_CORE_FIELDS!(
    Context, ContextField, CONTEXT {
        Parent => parent,
        NumVariables => num_variables,
        Variables => variables,
    }
);

DEFINE_CORE_FIELDS!(
    ContextScope, ContextScopeField, CONTEXT_SCOPE {
        NumVariables => num_variables,
        IsImplicit => is_implicit,
        Variables => variables,
    }
);

DEFINE_CORE_FIELDS!(
    UnlinkedCall, UnlinkedCallField, UNLINKED_CALL {
        TargetName => target_name,
        ArgsDescriptor => args_descriptor,
        CanPatchToMonomorphic => can_patch_to_monomorphic,
    }
);

DEFINE_CORE_FIELDS!(
    ObjectPool, ObjectPoolField, OBJECT_POOL {
        Length => length,
        Entries => entries,
    }
);

DEFINE_CORE_FIELDS!(
    ObjectPoolEntry, ObjectPoolEntryField, OBJECT_POOL_ENTRY {
        EntryBits => entry_bits,
        Value => value,
    }
);

DEFINE_CORE_FIELDS!(
    Mint, MintField, MINT {
        Value => value,
    }
);

DEFINE_CORE_FIELDS!(
    Double, DoubleField, DOUBLE {
        Value => value,
    }
);

DEFINE_CORE_FIELDS!(
    TypeArguments, TypeArgumentsField, TYPE_ARGUMENTS {
        Instantiations => instantiations,
        Length => length,
        Hash => hash,
        Nullability => nullability,
        Types => types,
    }
);

DEFINE_CORE_FIELDS!(
    TypeParameter, TypeParameterField, TYPE_PARAMETER {
        TypeTestStub => type_test_stub,
        Hash => hash,
        Owner => owner,
        Base => base,
        Index => index,
        Flags => flags,
    }
);

DEFINE_CORE_FIELDS!(
    Type, TypeField, TYPE {
        TypeTestStub => type_test_stub,
        Hash => hash,
        Arguments => arguments,
        Flags => flags,
    }
);

DEFINE_CORE_FIELDS!(
    TypeParameters, TypeParametersField, TYPE_PARAMETERS {
        Names => names,
        Flags => flags,
        Bounds => bounds,
        Defaults => defaults,
    }
);

DEFINE_CORE_FIELDS!(
    _String, _StringField, STRING {
        StringType => string_type,
        Hash => hash,
        Length => length,
        InternalStr => internal_str,
    }
);

DEFINE_CORE_FIELDS!(
    Array, ArrayField, ARRAY {
        TypeArguments => type_arguments,
        Length => length,
        Elements => elements,
    }
);

DEFINE_CORE_FIELDS!(
    AbstractType, AbstractTypeField, ABSTRACT_TYPE {
        TypeTestStub => type_test_stub,
        Hash => hash,
        Padding => padding,
        Flags => flags,
    }
);

DEFINE_CORE_FIELDS!(
    FunctionType, FunctionTypeField, FUNCTION_TYPE {
        TypeTestStub => type_test_stub,
        Hash => hash,
        TypeParameters => type_parameters,
        ResultType => result_type,
        ParameterTypes => parameter_types,
        NamedParameterNames => named_parameter_names,
        Flags => flags,
        PackedParameterCounts => packed_parameter_counts,
        PackedTypeParameterCounts => packed_type_parameter_counts,
    }
);

DEFINE_CORE_FIELDS!(
    Closure, ClosureField, CLOSURE {
        InstantiatorTypeArguments => instantiator_type_arguments,
        FunctionTypeArguments => function_type_arguments,
        DelayedTypeArguments => delayed_type_arguments,
        Function => function,
        Context => context,
        Hash => hash,
    }
);

DEFINE_CORE_FIELDS!(
    Instance, InstanceField, INSTANCE {
        NextFieldOffsetInWords => next_field_offset_in_words,
        InstanceSizeInWords => instance_size_in_words,
        UnboxedFieldsBitmap => unboxed_fields_bitmap,
        Fields => fields,
    }
);

DEFINE_CORE_FIELDS!(
    WeakArray, WeakArrayField, WEAK_ARRAY {
        NextSeenByGc => next_seen_by_gc,
        Length => length,
        Elements => elements,
    }
);

DEFINE_CORE_FIELDS!(
    TypedDataBase, TypedDataBaseField, TYPED_DATA_BASE {
        Length => length,
        Padding => padding,
    }
);

DEFINE_CORE_FIELDS!(
    TypedData, TypedDataField, TYPED_DATA {
        Length => length,
        Data => data,
    }
);

DEFINE_CORE_FIELDS!(
    TypedDataView, TypedDataViewField, TYPED_DATA_VIEW {
        Length => length,
        TypedData => typed_data,
        OffsetInBytes => offset_in_bytes,
    }
);

DEFINE_CORE_FIELDS!(
    GrowableObjectArray, GrowableObjectArrayField, GROWABLE_OBJECT_ARRAY {
        TypeArguments => type_arguments,
        Length => length,
        Data => data,
    }
);

DEFINE_CORE_FIELDS!(
    Code, CodeField, CODE {
        EntryPoint => entry_point,
        MonomorphicEntryPoint => monomorphic_entry_point,
        UncheckedEntryPoint => unchecked_entry_point,
        MonomorphicUncheckedEntryPoint => monomorphic_unchecked_entry_point,
        HasMonomorphicEntrypoint => has_monomorphic_entrypoint,
        ObjectPool => object_pool,
        Instructions => instructions,
        Owner => owner,
        ExceptionHandlers => exception_handlers,
        PcDescriptors => pc_descriptors,
        CatchEntry => catch_entry,
        CompressedStackmaps => compressed_stackmaps,
        InlinedIdToFunction => inlined_id_to_function,
        CodeSourceMap => code_source_map,
        StateBits => state_bits,
        InstructionsLength => instructions_length_,
    }
);

DEFINE_CORE_FIELDS!(
    LoadingUnit, LoadingUnitField, LOADING_UNIT {
        Parent => parent,
        BaseObjects => base_objects,
        PackedFields => packed_fields,
    }
);

DEFINE_CORE_FIELDS!(
    ICData, ICDataField, IC_DATA {
        TargetName => target_name,
        ArgsDescriptor => args_descriptor,
        Entries => entries,
        StateBits => state_bits,
    }
);

DEFINE_CORE_FIELDS!(
    MegamorphicCache, MegamorphicCacheField, MEGAMORPHIC_CACHE {
        TargetName => target_name,
        ArgsDescriptor => args_descriptor,
        Buckets => buckets,
        Mask => mask,
        FilledEntryCount => filled_entry_count,
    }
);

DEFINE_CORE_FIELDS!(
    SubtypeTestCache, SubtypeTestCacheField, SUBTYPE_TEST_CACHE {
        Cache => cache,
        NumInputs => num_inputs,
        NumOccupied => num_occupied,
    }
);

DEFINE_CORE_FIELDS!(
    LanguageError, LanguageErrorField, LANGUAGE_ERROR {
        PreviousError => previous_error,
        Script => script,
        Message => message,
        FormattedMessage => formatted_message,
        TokenPos => token_pos,
        ReportAfterToken => report_after_token,
        Kind => kind,
    }
);

DEFINE_CORE_FIELDS!(
    UnhandledException, UnhandledExceptionField, UNHANDLED_EXCEPTION {
        Exception => exception,
        Stacktrace => stacktrace,
    }
);

DEFINE_CORE_FIELDS!(
    LibraryPrefix, LibraryPrefixField, LIBRARY_PREFIX {
        Name => name,
        Imports => imports,
        Importer => importer,
        NumImports => num_imports,
        IsDeferredLoad => is_deferred_load,
    }
);

DEFINE_CORE_FIELDS!(
    RecordType, RecordTypeField, RECORD_TYPE {
        TypeTestStub => type_test_stub,
        Hash => hash,
        Shape => shape,
        FieldTypes => field_types,
        Flags => flags,
    }
);

DEFINE_CORE_FIELDS!(
    Int32x4, Int32x4Field, INT32X4 {
        Value => value,
    }
);

DEFINE_CORE_FIELDS!(
    ExternalTypedData, ExternalTypedDataField, EXTERNAL_TYPED_DATA {
        Length => length,
        Data => data,
    }
);

DEFINE_CORE_FIELDS!(
    StackTrace, StackTraceField, STACK_TRACE {
        AsyncLink => async_link,
        CodeArray => code_array,
        PcOffsetArray => pc_offset_array,
    }
);

DEFINE_CORE_FIELDS!(
    RegExp, RegExpField, REG_EXP {
        CaptureNameMap => capture_name_map,
        Pattern => pattern,
        OneByte => one_byte,
        TwoByte => two_byte,
        OneByteSticky => one_byte_sticky,
        TwoByteSticky => two_byte_sticky,
        NumOneByteRegisters => num_one_byte_registers,
        NumTwoByteRegisters => num_two_byte_registers,
        TypeFlags => type_flags,
    }
);

DEFINE_CORE_FIELDS!(
    WeakProperty, WeakPropertyField, WEAK_PROPERTY {
        Key => key,
        Value => value,
    }
);

DEFINE_CORE_FIELDS!(
    Map, MapField, MAP {
        TypeArguments => type_arguments,
        HashMask => hash_mask,
        Data => data,
        UsedData => used_data,
        DeletedKeys => deleted_keys,
        Index => index,
    }
);

DEFINE_CORE_FIELDS!(
    Set, SetField, SET {
        TypeArguments => type_arguments,
        HashMask => hash_mask,
        Data => data,
        UsedData => used_data,
        DeletedKeys => deleted_keys,
        Index => index,
    }
);

DEFINE_CORE_FIELDS!(
    Float32x4, Float32x4Field, FLOAT32X4 {
        Value => value,
    }
);

DEFINE_CORE_FIELDS!(
    Float64x2, Float64x2Field, FLOAT64X2 {
        Value => value,
    }
);

DEFINE_CORE_FIELDS!(
    ConstMap, ConstMapField, CONST_MAP {
        TypeArguments => type_arguments,
        HashMask => hash_mask,
        Data => data,
        UsedData => used_data,
        DeletedKeys => deleted_keys,
        Index => index,
    }
);

DEFINE_CORE_FIELDS!(
    ConstSet, ConstSetField, CONST_SET {
        TypeArguments => type_arguments,
        HashMask => hash_mask,
        Data => data,
        UsedData => used_data,
        DeletedKeys => deleted_keys,
        Index => index,
    }
);

DEFINE_CORE_FIELDS!(
    Record, RecordField, RECORD {
        Shape => shape,
        Padding => padding,
        NumFields => num_fields,
        Fields => fields,
    }
);

DEFINE_CORE_FIELDS!(
    ImmutableArray, ImmutableArrayField, IMMUTABLE_ARRAY {
        TypeArguments => type_arguments,
        Length => length,
        Elements => elements,
    }
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocabulary_deserializes_semantic_field_names() {
        assert_eq!(
            serde_json::from_str::<FunctionField>("\"name\"").unwrap(),
            FunctionField::Name
        );
        assert_eq!(
            serde_json::from_str::<ExceptionHandlerInfoField>("\"handler_pc_offset\"").unwrap(),
            ExceptionHandlerInfoField::HandlerPcOffset
        );
        assert!(serde_json::from_str::<FunctionField>("\"handler_pc_offset\"").is_err());
    }

    #[test]
    fn unsigned_fields_preserve_signed_encoding_bits() {
        let mut class = crate::raw_object::Class::default();
        class
            .set_core_field(ClassField::StateBits, PropType::Signed(-1))
            .unwrap();
        assert_eq!(class.state_bits, u32::MAX);
        assert!(class
            .set_core_field(ClassField::NumNativeFields, PropType::Signed(-32769))
            .is_err());
        assert!(class
            .set_core_field(ClassField::NumNativeFields, PropType::Unsigned(65536))
            .is_err());
        class
            .set_core_field(ClassField::NumNativeFields, PropType::Signed(-1))
            .unwrap();
        assert_eq!(class.num_native_fields, u16::MAX);
    }

    #[test]
    fn raw_bytes_preserve_signed_byte_fields_and_nonzero_bools() {
        let mut info = crate::raw_object::ExceptionHandlerInfo::default();
        info.set_core_field(
            ExceptionHandlerInfoField::NeedsStacktrace,
            PropType::Byte(255),
        )
        .unwrap();
        assert_eq!(info.needs_stacktrace, -1);

        let mut call = crate::raw_object::UnlinkedCall::default();
        call.set_core_field(
            UnlinkedCallField::CanPatchToMonomorphic,
            PropType::Byte(2),
        )
        .unwrap();
        assert!(call.can_patch_to_monomorphic);
    }

    #[test]
    fn double_fields_use_raw_bits_instead_of_numeric_conversion() {
        let mut double = crate::raw_object::Double::default();
        let bits = (-12.5_f64).to_bits();
        double
            .set_core_field(DoubleField::Value, PropType::RawU64(bits))
            .unwrap();
        assert_eq!(double.value.to_bits(), bits);
        double
            .set_core_field(DoubleField::Value, PropType::Signed(bits as i64))
            .unwrap();
        assert_eq!(double.value.to_bits(), bits);
        assert!(double
            .set_core_field(DoubleField::Value, PropType::Unsigned(bits))
            .is_err());
    }

    #[test]
    fn collection_fields_are_built_directly_instead_of_scalar_assigned() {
        let mut array = crate::raw_object::Array::default();
        assert!(array
            .set_core_field(
                ArrayField::Elements,
                PropType::Array(vec![PropType::RefId(42)])
            )
            .is_err());
        array
            .core_field_mut(ArrayField::Elements)
            .downcast_mut::<Vec<u32>>()
            .unwrap()
            .push(42);
        assert_eq!(array.elements, vec![42]);
    }
}
