type Smi = i32; // Using i64 for Smi fields to be decompressed

// --- Fixed-Size Objects with defined fields ---

#[derive(Default)]
pub struct Mint {
    pub value: i64,
}

#[derive(Default)]
pub struct Double {
    pub value: f64,
}

#[derive(Default)]
pub struct TypeArguments<'a> {
    pub instantiations: Option<&'a mut Array<'a>>, // ArrayPtr
    pub length: Smi,                               // Smi
    pub hash: Smi,                                 // Smi
    pub nullability: Smi,                          // Smi
}

#[derive(Default)]
pub struct TypeParameter<'a> {
    pub owner: Option<&'a mut Object<'a>>, // ObjectPtr
    pub base: i16,
    pub index: i16,
}

#[derive(Default)]
pub struct Type<'a> {
    pub arguments: Option<&'a mut TypeArguments<'a>>, // TypeArgumentsPtr
}

#[derive(Default)]
pub struct TypeParameters<'a> {
    pub names: Option<&'a mut Array<'a>>,            // ArrayPtr
    pub flags: Option<&'a mut Array<'a>>,            // ArrayPtr
    pub bounds: Option<&'a mut TypeArguments<'a>>,   // TypeArgumentsPtr
    pub defaults: Option<&'a mut TypeArguments<'a>>, // TypeArgumentsPtr
}

#[derive(Default)]
pub struct PatchClass<'a> {
    pub wrapped_class: Option<&'a mut Class<'a>>, // ClassPtr
    pub script: Option<&'a mut Script<'a>>,       // ScriptPtr
    pub kernel_program_info: Option<&'a mut KernelProgramInfo<'a>>, // KernelProgramInfoPtr
}

#[derive(Default)]
pub struct ClosureData<'a> {
    pub context_scope: Option<&'a mut ContextScope>, // ContextScopePtr
    pub parent_function: Option<&'a mut Function<'a>>, // FunctionPtr
    pub closure: Option<&'a mut Closure<'a>>,        // ClosurePtr
    pub packed_fields: u32,
}

#[derive(Default)]
pub struct FfiTrampolineData<'a> {
    pub signature_type: Option<&'a mut Type<'a>>, // TypePtr
    pub c_signature: Option<&'a mut FunctionType<'a>>, // FunctionTypePtr
    pub callback_target: Option<&'a mut Function<'a>>, // FunctionPtr
    pub callback_exceptional_return: Option<&'a mut Instance<'a>>, // InstancePtr
    pub ffi_function_kind: u8,
    pub callback_id: i32,
}

#[derive(Default)]
pub struct Field<'a> {
    pub name: _String,                                      // StringPtr -> Raw String
    pub owner: Option<&'a mut Object<'a>>,                  // ObjectPtr
    pub type_field: Option<&'a mut AbstractType<'a>>,       // AbstractTypePtr
    pub initializer_function: Option<&'a mut Function<'a>>, // FunctionPtr
    pub host_offset_or_field_id: Smi,                       // Smi
    pub guarded_list_length: Smi,                           // Smi
    pub exact_type: Option<&'a mut AbstractType<'a>>,       // AbstractTypePtr
    pub dependent_code: Option<&'a mut WeakArray<'a>>,      // WeakArrayPtr
    pub kernel_offset: i32,
    pub guarded_list_length_in_object_offset: i8,
    pub static_type_exactness_state: i8,
    pub target_offset: i32,
    pub kind_bits: u32,
}

#[derive(Default)]
pub struct Namespace<'a> {
    pub target: Option<&'a mut Library<'a>>,   // LibraryPtr
    pub show_names: Option<&'a mut Array<'a>>, // ArrayPtr
    pub hide_names: Option<&'a mut Array<'a>>, // ArrayPtr
    pub owner: Option<&'a mut Library<'a>>,    // LibraryPtr
}

#[derive(Default)]
pub struct KernelProgramInfo<'a> {
    pub kernel_component: Option<&'a mut TypedDataBase<'a>>, // TypedDataBasePtr
    pub string_offsets: Option<&'a mut TypedData<'a>>,       // TypedDataPtr
    pub string_data: Option<&'a mut TypedDataView<'a>>,      // TypedDataViewPtr
    pub canonical_names: Option<&'a mut TypedData<'a>>,      // TypedDataPtr
    pub metadata_payloads: Option<&'a mut TypedDataView<'a>>, // TypedDataViewPtr
    pub metadata_mappings: Option<&'a mut TypedDataView<'a>>, // TypedDataViewPtr
    pub scripts: Option<&'a mut Array<'a>>,                  // ArrayPtr
    pub constants: Option<&'a mut Array<'a>>,                // ArrayPtr
    pub constants_table: Option<&'a mut TypedDataView<'a>>,  // TypedDataViewPtr
    pub libraries_cache: Option<&'a mut Array<'a>>,          // ArrayPtr
    pub classes_cache: Option<&'a mut Array<'a>>,            // ArrayPtr
}

#[derive(Default)]
pub struct ExceptionHandlers<'a> {
    pub handled_types_data: Option<&'a mut Array<'a>>, // ArrayPtr
    pub packed_fields: u32,
}

#[derive(Default)]
pub struct Context<'a> {
    pub parent: Option<&'a mut Context<'a>>, // ContextPtr
    pub num_variables: i32,
}

#[derive(Default)]
pub struct UnlinkedCall {
    pub can_patch_to_monomorphic: bool,
}

#[derive(Default)]
pub struct _String {
    // added underscore so there's no conflict between this type and rust's _String
    pub hash: Smi,   // Smi
    pub length: Smi, // Smi
    pub inner_string: String,
}

#[derive(Default)]
pub struct Class<'a> {
    pub name: _String,                                       // StringPtr -> Raw String
    pub user_name: _String,                                  // StringPtr -> Raw String
    pub functions: Option<&'a mut Array<'a>>,                // ArrayPtr
    pub functions_hash_table: Option<&'a mut Array<'a>>,     // ArrayPtr
    pub fields: Option<&'a mut Array<'a>>,                   // ArrayPtr
    pub offset_in_words_to_field: Option<&'a mut Array<'a>>, // ArrayPtr
    pub interfaces: Option<&'a mut Array<'a>>,               // ArrayPtr
    pub script: Option<&'a mut Script<'a>>,                  // ScriptPtr
    pub library: Option<&'a mut Library<'a>>,                // LibraryPtr
    pub type_parameters: Option<&'a mut TypeParameters<'a>>, // TypeParametersPtr
    pub super_type: Option<&'a mut Type<'a>>,                // TypePtr
    pub constants: Option<&'a mut Array<'a>>,                // ArrayPtr
    pub declaration_type: Option<&'a mut Type<'a>>,          // TypePtr
    pub invocation_dispatcher_cache: Option<&'a mut Array<'a>>, // ArrayPtr
    pub direct_implementors: Option<&'a mut GrowableObjectArray<'a>>, // GrowableObjectArrayPtr
    pub direct_subclasses: Option<&'a mut GrowableObjectArray<'a>>, // GrowableObjectArrayPtr
    pub declaration_instance_type_arguments: Option<&'a mut TypeArguments<'a>>, // TypeArgumentsPtr
    pub allocation_stub: Option<&'a mut Code<'a>>,           // CodePtr
    pub dependent_code: Option<&'a mut WeakArray<'a>>,       // WeakArrayPtr
    pub num_type_arguments: i16,
    pub num_native_fields: u16,
    pub state_bits: u32,
    pub host_instance_size_in_words: i32,
    pub host_type_arguments_field_offset_in_words: i32,
    pub host_next_field_offset_in_words: i32,
    pub target_instance_size_in_words: i32,
    pub target_type_arguments_field_offset_in_words: i32,
    pub target_next_field_offset_in_words: i32,
    pub kernel_offset: i32,
}

#[derive(Default)]
pub struct Function<'a> {
    pub name: _String,                               // StringPtr -> Raw String
    pub owner: Option<&'a mut Object<'a>>,           // ObjectPtr
    pub signature: Option<&'a mut FunctionType<'a>>, // FunctionTypePtr
    pub data: Option<&'a mut Object<'a>>,            // ObjectPtr
    pub ic_data_array_or_bytecode: Option<&'a mut Object<'a>>, // ObjectPtr
    pub code: Option<&'a mut Code<'a>>,              // CodePtr
    pub positional_parameter_names: Option<&'a mut Array<'a>>, // ArrayPtr
    pub unoptimized_code: Option<&'a mut Code<'a>>,  // CodePtr
    pub bitmap: u64,
    pub kernel_offset: i32,
    pub kind_tag: u32,
}

#[derive(Default)]
pub struct Library<'a> {
    pub name: _String,                             // StringPtr -> Raw String
    pub url: _String,                              // StringPtr -> Raw String
    pub private_key: _String,                      // StringPtr -> Raw String
    pub dictionary: Option<&'a mut Array<'a>>,     // ArrayPtr
    pub metadata: Option<&'a mut Array<'a>>,       // ArrayPtr
    pub toplevel_class: Option<&'a mut Class<'a>>, // ClassPtr
    pub used_scripts: Option<&'a mut GrowableObjectArray<'a>>, // GrowableObjectArrayPtr
    pub loading_unit: Option<&'a mut LoadingUnit<'a>>, // LoadingUnitPtr
    pub imports: Option<&'a mut Array<'a>>,        // ArrayPtr
    pub exports: Option<&'a mut Array<'a>>,        // ArrayPtr
    pub dependencies: Option<&'a mut Array<'a>>,   // ArrayPtr
    pub kernel_program_info: Option<&'a mut KernelProgramInfo<'a>>, // KernelProgramInfoPtr
    pub loaded_scripts: Option<&'a mut Array<'a>>, // ArrayPtr
    pub num_imports: u16,
    pub load_state: i8,
    pub flags: u8,
    pub kernel_library_index: i32,
}

#[derive(Default)]
pub struct ContextScope {
    pub num_variables: i32,
    pub is_implicit: bool,
}

// Fieldless classes
// These classes either have no additional payload fields beyond the standard
// instance headers or their payloads are purely variable-length or dynamically read/overlayed on top of a byte stream

#[derive(Default)]
pub struct CodeSourceMap;

#[derive(Default)]
pub struct CompressedStackMaps;

#[derive(Default)]
pub struct PcDescriptors;

#[derive(Default)]
pub struct ObjectPool;

// Placeholder structs for references used above that aren't defined yet
// it wouldn't compile without this, though for now they remain unimplemented...
#[derive(Default)]
pub struct Array<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}
#[derive(Default)]
pub struct Object<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}
#[derive(Default)]
pub struct AbstractType<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}
#[derive(Default)]
pub struct FunctionType<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}
#[derive(Default)]
pub struct Script<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}
#[derive(Default)]
pub struct Closure<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}
#[derive(Default)]
pub struct Instance<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}
#[derive(Default)]
pub struct WeakArray<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}
#[derive(Default)]
pub struct TypedDataBase<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}
#[derive(Default)]
pub struct TypedData<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}
#[derive(Default)]
pub struct TypedDataView<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}
#[derive(Default)]
pub struct GrowableObjectArray<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}
#[derive(Default)]
pub struct Code<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}
#[derive(Default)]
pub struct LoadingUnit<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}

#[derive(Default)]
pub struct ICData;
#[derive(Default)]
pub struct MegamorphicCache;
#[derive(Default)]
pub struct SubtypeTestCache;
#[derive(Default)]
pub struct LanguageError;
#[derive(Default)]
pub struct UnhandledException;
#[derive(Default)]
pub struct LibraryPrefix;
#[derive(Default)]
pub struct RecordType;
#[derive(Default)]
pub struct Int32x4;
#[derive(Default)]
pub struct ExternalTypedData;
#[derive(Default)]
pub struct StackTrace;
#[derive(Default)]
pub struct RegExp;
#[derive(Default)]
pub struct WeakProperty;
#[derive(Default)]
pub struct Map;
#[derive(Default)]
pub struct Set;
#[derive(Default)]
pub struct Float32x4;
#[derive(Default)]
pub struct Float64x2;
#[derive(Default)]
pub struct ConstMap;
#[derive(Default)]
pub struct ConstSet;
#[derive(Default)]
pub struct Record;
#[derive(Default)]
pub struct ImmutableArray;
#[derive(Default)]
pub struct OneByteString;
#[derive(Default)]
pub struct TwoByteString;
