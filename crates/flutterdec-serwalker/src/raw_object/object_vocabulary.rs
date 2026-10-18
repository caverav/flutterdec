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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectField {
    Tags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClassField {
    Id,
    IsPredefined,
    Name,
    UserName,
    Functions,
    FunctionsHashTable,
    Fields,
    OffsetInWordsToField,
    Interfaces,
    Script,
    Library,
    TypeParameters,
    SuperType,
    Constants,
    DeclarationType,
    InvocationDispatcherCache,
    DirectImplementors,
    DirectSubclasses,
    DeclarationInstanceTypeArguments,
    AllocationStub,
    DependentCode,
    NumNativeFields,
    StateBits,
    KernelOffset,
    NumTypeArguments,
    HostInstanceSizeInWords,
    HostTypeArgumentsFieldOffsetInWords,
    HostNextFieldOffsetInWords,
    TargetInstanceSizeInWords,
    TargetTypeArgumentsFieldOffsetInWords,
    TargetNextFieldOffsetInWords,
    UnboxedFieldsBitmap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PatchClassField {
    WrappedClass,
    Script,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionField {
    EntryPoint,
    UncheckedEntryPoint,
    Name,
    Owner,
    Signature,
    Data,
    CodeIndex,
    TokenPos,
    KindTag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClosureDataField {
    ContextScope,
    ParentFunction,
    Closure,
    PackedFields,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FfiTrampolineDataField {
    SignatureType,
    CSignature,
    CallbackTarget,
    CallbackExceptionalReturn,
    CallbackId,
    FfiFunctionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldField {
    Name,
    Owner,
    TypeField,
    InitializerFunction,
    HostOffsetOrFieldId,
    TokenPos,
    EndTokenPos,
    GuardedCid,
    IsNullable,
    KindBits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScriptField {
    Url,
    KernelScriptIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LibraryField {
    Name,
    Url,
    PrivateKey,
    Dictionary,
    Metadata,
    ToplevelClass,
    UsedScripts,
    LoadingUnit,
    Imports,
    Exports,
    Index,
    NumImports,
    LoadState,
    Flags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamespaceField {
    Target,
    ShowNames,
    HideNames,
    Owner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KernelProgramInfoField {
    KernelComponent,
    StringOffsets,
    StringData,
    CanonicalNames,
    MetadataPayloads,
    MetadataMappings,
    Scripts,
    Constants,
    ConstantsTable,
    LibrariesCache,
    ClassesCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodeSourceMapField {
    Length,
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompressedStackMapsField {
    Length,
    FlagsAndSize,
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcDescriptorsField {
    Length,
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExceptionHandlersField {
    HandledTypesData,
    PackedFields,
    NumEntries,
    Entries,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExceptionHandlerInfoField {
    HandlerPcOffset,
    OuterTryIndex,
    NeedsStacktrace,
    HasCatchAll,
    IsGenerated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextField {
    Parent,
    NumVariables,
    Variables,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextScopeField {
    NumVariables,
    IsImplicit,
    Variables,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnlinkedCallField {
    TargetName,
    ArgsDescriptor,
    CanPatchToMonomorphic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectPoolField {
    Length,
    Entries,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectPoolEntryField {
    EntryBits,
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MintField {
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DoubleField {
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeArgumentsField {
    Instantiations,
    Length,
    Hash,
    Nullability,
    Types,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeParameterField {
    TypeTestStub,
    Hash,
    Owner,
    Base,
    Index,
    Flags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeField {
    TypeTestStub,
    Hash,
    Arguments,
    Flags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeParametersField {
    Names,
    Flags,
    Bounds,
    Defaults,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum _StringField {
    StringType,
    Hash,
    Length,
    InternalStr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArrayField {
    TypeArguments,
    Length,
    Elements,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AbstractTypeField {
    TypeTestStub,
    Hash,
    Padding,
    Flags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionTypeField {
    TypeTestStub,
    Hash,
    TypeParameters,
    ResultType,
    ParameterTypes,
    NamedParameterNames,
    Flags,
    PackedParameterCounts,
    PackedTypeParameterCounts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClosureField {
    InstantiatorTypeArguments,
    FunctionTypeArguments,
    DelayedTypeArguments,
    Function,
    Context,
    Hash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstanceField {
    NextFieldOffsetInWords,
    InstanceSizeInWords,
    UnboxedFieldsBitmap,
    Fields,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WeakArrayField {
    NextSeenByGc,
    Length,
    Elements,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypedDataBaseField {
    Length,
    Padding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypedDataField {
    Length,
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypedDataViewField {
    Length,
    TypedData,
    OffsetInBytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GrowableObjectArrayField {
    TypeArguments,
    Length,
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodeField {
    EntryPoint,
    MonomorphicEntryPoint,
    UncheckedEntryPoint,
    MonomorphicUncheckedEntryPoint,
    HasMonomorphicEntrypoint,
    ObjectPool,
    Instructions,
    Owner,
    ExceptionHandlers,
    PcDescriptors,
    CatchEntry,
    CompressedStackmaps,
    InlinedIdToFunction,
    CodeSourceMap,
    StateBits,
    InstructionsLength,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LoadingUnitField {
    Parent,
    BaseObjects,
    PackedFields,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ICDataField {
    TargetName,
    ArgsDescriptor,
    Entries,
    StateBits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MegamorphicCacheField {
    TargetName,
    ArgsDescriptor,
    Buckets,
    Mask,
    FilledEntryCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubtypeTestCacheField {
    Cache,
    NumInputs,
    NumOccupied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageErrorField {
    PreviousError,
    Script,
    Message,
    FormattedMessage,
    TokenPos,
    ReportAfterToken,
    Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnhandledExceptionField {
    Exception,
    Stacktrace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LibraryPrefixField {
    Name,
    Imports,
    Importer,
    NumImports,
    IsDeferredLoad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordTypeField {
    TypeTestStub,
    Hash,
    Shape,
    FieldTypes,
    Flags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Int32x4Field {
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExternalTypedDataField {
    Length,
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StackTraceField {
    AsyncLink,
    CodeArray,
    PcOffsetArray,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegExpField {
    CaptureNameMap,
    Pattern,
    OneByte,
    TwoByte,
    OneByteSticky,
    TwoByteSticky,
    NumOneByteRegisters,
    NumTwoByteRegisters,
    TypeFlags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WeakPropertyField {
    Key,
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapField {
    TypeArguments,
    HashMask,
    Data,
    UsedData,
    DeletedKeys,
    Index,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SetField {
    TypeArguments,
    HashMask,
    Data,
    UsedData,
    DeletedKeys,
    Index,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Float32x4Field {
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Float64x2Field {
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstMapField {
    TypeArguments,
    HashMask,
    Data,
    UsedData,
    DeletedKeys,
    Index,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstSetField {
    TypeArguments,
    HashMask,
    Data,
    UsedData,
    DeletedKeys,
    Index,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordField {
    Shape,
    Padding,
    NumFields,
    Fields,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImmutableArrayField {
    TypeArguments,
    Length,
    Elements,
}
