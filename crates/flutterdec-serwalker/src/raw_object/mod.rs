// raw_object/mod.rs

type Smi = i64; // Using i64 for Smi fields to be decompressed

// --- Fixed-Size Objects with defined fields ---

pub struct Mint {
    pub value: i64,
}

pub struct Double {
    pub value: f64,
}

pub struct TypeArguments<'a> {
    pub instantiations: Option<&'a mut Array<'a>>, // ArrayPtr
    pub length: Smi,                               // Smi
    pub hash: Smi,                                 // Smi
    pub nullability: Smi,                          // Smi
}

pub struct TypeParameter<'a> {
    pub owner: Option<&'a mut Object<'a>>, // ObjectPtr
    pub base: i16,
    pub index: i16,
}

pub struct Type<'a> {
    pub arguments: Option<&'a mut TypeArguments<'a>>, // TypeArgumentsPtr
}

pub struct TypeParameters<'a> {
    pub names: Option<&'a mut Array<'a>>,            // ArrayPtr
    pub flags: Option<&'a mut Array<'a>>,            // ArrayPtr
    pub bounds: Option<&'a mut TypeArguments<'a>>,   // TypeArgumentsPtr
    pub defaults: Option<&'a mut TypeArguments<'a>>, // TypeArgumentsPtr
}

pub struct PatchClass<'a> {
    pub wrapped_class: Option<&'a mut Class<'a>>,                   // ClassPtr
    pub script: Option<&'a mut Script<'a>>,                          // ScriptPtr
    pub kernel_program_info: Option<&'a mut KernelProgramInfo<'a>>, // KernelProgramInfoPtr
}

pub struct ClosureData<'a> {
    pub context_scope: Option<&'a mut ContextScope>,   // ContextScopePtr
    pub parent_function: Option<&'a mut Function<'a>>, // FunctionPtr
    pub closure: Option<&'a mut Closure<'a>>,          // ClosurePtr
    pub packed_fields: u32,
}

pub struct FfiTrampolineData<'a> {
    pub signature_type: Option<&'a mut Type<'a>>,                  // TypePtr
    pub c_signature: Option<&'a mut FunctionType<'a>>,             // FunctionTypePtr
    pub callback_target: Option<&'a mut Function<'a>>,             // FunctionPtr
    pub callback_exceptional_return: Option<&'a mut Instance<'a>>, // InstancePtr
    pub ffi_function_kind: u8,
    pub callback_id: i32,
}

pub struct Field<'a> {
    pub name: String,                    // StringPtr -> Raw String
    pub owner: Option<&'a mut Object<'a>>,            // ObjectPtr
    pub type_field: Option<&'a mut AbstractType<'a>>, // AbstractTypePtr
    pub initializer_function: Option<&'a mut Function<'a>>, // FunctionPtr
    pub host_offset_or_field_id: Smi,                 // Smi
    pub guarded_list_length: Smi,                     // Smi
    pub exact_type: Option<&'a mut AbstractType<'a>>, // AbstractTypePtr
    pub dependent_code: Option<&'a mut WeakArray<'a>>, // WeakArrayPtr
    pub kernel_offset: i32,
    pub guarded_list_length_in_object_offset: i8,
    pub static_type_exactness_state: i8,
    pub target_offset: i32,
    pub kind_bits: u32,
}

pub struct Namespace<'a> {
    pub target: Option<&'a mut Library<'a>>,     // LibraryPtr
    pub show_names: Option<&'a mut Array<'a>>,   // ArrayPtr
    pub hide_names: Option<&'a mut Array<'a>>,   // ArrayPtr
    pub owner: Option<&'a mut Library<'a>>,      // LibraryPtr
}

pub struct KernelProgramInfo<'a> {
    pub kernel_component: Option<&'a mut TypedDataBase<'a>>,  // TypedDataBasePtr
    pub string_offsets: Option<&'a mut TypedData<'a>>,        // TypedDataPtr
    pub string_data: Option<&'a mut TypedDataView<'a>>,       // TypedDataViewPtr
    pub canonical_names: Option<&'a mut TypedData<'a>>,       // TypedDataPtr
    pub metadata_payloads: Option<&'a mut TypedDataView<'a>>, // TypedDataViewPtr
    pub metadata_mappings: Option<&'a mut TypedDataView<'a>>, // TypedDataViewPtr
    pub scripts: Option<&'a mut Array<'a>>,                   // ArrayPtr
    pub constants: Option<&'a mut Array<'a>>,                 // ArrayPtr
    pub constants_table: Option<&'a mut TypedDataView<'a>>,   // TypedDataViewPtr
    pub libraries_cache: Option<&'a mut Array<'a>>,           // ArrayPtr
    pub classes_cache: Option<&'a mut Array<'a>>,             // ArrayPtr
}

pub struct ExceptionHandlers<'a> {
    pub handled_types_data: Option<&'a mut Array<'a>>, // ArrayPtr
    pub packed_fields: u32,
}

pub struct Context<'a> {
    pub parent: Option<&'a mut Context<'a>>, // ContextPtr
    pub num_variables: i32,
}

pub struct UnlinkedCall {
    pub can_patch_to_monomorphic: bool,
}

pub struct String {
    pub hash: Smi,   // Smi
    pub length: Smi, // Smi
}

pub struct Class<'a> {
    pub name: String,                                // StringPtr -> Raw String
    pub user_name: String,                           // StringPtr -> Raw String
    pub functions: Option<&'a mut Array<'a>>,                     // ArrayPtr
    pub functions_hash_table: Option<&'a mut Array<'a>>,          // ArrayPtr
    pub fields: Option<&'a mut Array<'a>>,                        // ArrayPtr
    pub offset_in_words_to_field: Option<&'a mut Array<'a>>,      // ArrayPtr
    pub interfaces: Option<&'a mut Array<'a>>,                    // ArrayPtr
    pub script: Option<&'a mut Script<'a>>,                       // ScriptPtr
    pub library: Option<&'a mut Library<'a>>,                     // LibraryPtr
    pub type_parameters: Option<&'a mut TypeParameters<'a>>,      // TypeParametersPtr
    pub super_type: Option<&'a mut Type<'a>>,                     // TypePtr
    pub constants: Option<&'a mut Array<'a>>,                     // ArrayPtr
    pub declaration_type: Option<&'a mut Type<'a>>,               // TypePtr
    pub invocation_dispatcher_cache: Option<&'a mut Array<'a>>,   // ArrayPtr
    pub direct_implementors: Option<&'a mut GrowableObjectArray<'a>>, // GrowableObjectArrayPtr
    pub direct_subclasses: Option<&'a mut GrowableObjectArray<'a>>, // GrowableObjectArrayPtr
    pub declaration_instance_type_arguments: Option<&'a mut TypeArguments<'a>>, // TypeArgumentsPtr
    pub allocation_stub: Option<&'a mut Code<'a>>,                // CodePtr
    pub dependent_code: Option<&'a mut WeakArray<'a>>,            // WeakArrayPtr
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

pub struct Function<'a> {
    pub name: String,                       // StringPtr -> Raw String
    pub owner: Option<&'a mut Object<'a>>,               // ObjectPtr
    pub signature: Option<&'a mut FunctionType<'a>>,     // FunctionTypePtr
    pub data: Option<&'a mut Object<'a>>,                // ObjectPtr
    pub ic_data_array_or_bytecode: Option<&'a mut Object<'a>>, // ObjectPtr
    pub code: Option<&'a mut Code<'a>>,                  // CodePtr
    pub positional_parameter_names: Option<&'a mut Array<'a>>, // ArrayPtr
    pub unoptimized_code: Option<&'a mut Code<'a>>,      // CodePtr
    pub bitmap: u64,
    pub kernel_offset: i32,
    pub kind_tag: u32,
}

pub struct Library<'a> {
    pub name: String,                // StringPtr -> Raw String
    pub url: String,                 // StringPtr -> Raw String
    pub private_key: String,         // StringPtr -> Raw String
    pub dictionary: Option<&'a mut Array<'a>>,    // ArrayPtr
    pub metadata: Option<&'a mut Array<'a>>,      // ArrayPtr
    pub toplevel_class: Option<&'a mut Class<'a>>, // ClassPtr
    pub used_scripts: Option<&'a mut GrowableObjectArray<'a>>, // GrowableObjectArrayPtr
    pub loading_unit: Option<&'a mut LoadingUnit<'a>>, // LoadingUnitPtr
    pub imports: Option<&'a mut Array<'a>>,       // ArrayPtr
    pub exports: Option<&'a mut Array<'a>>,       // ArrayPtr
    pub dependencies: Option<&'a mut Array<'a>>,  // ArrayPtr
    pub kernel_program_info: Option<&'a mut KernelProgramInfo<'a>>, // KernelProgramInfoPtr
    pub loaded_scripts: Option<&'a mut Array<'a>>, // ArrayPtr
    pub num_imports: u16,
    pub load_state: i8,
    pub flags: u8,
    pub kernel_library_index: i32,
}

pub struct ContextScope {
    pub num_variables: i32,
    pub is_implicit: bool,
}

// Fieldless classes
// These classes either have no additional payload fields beyond the standard
// instance headers or their payloads are purely variable-length or dynamically read/overlayed on top of a byte stream

pub struct CodeSourceMap;
pub struct CompressedStackMaps;
pub struct PcDescriptors;
pub struct ObjectPool;
pub struct OneByteString;
pub struct TwoByteString;

// Placeholder structs for references used above that aren't defined yet
// it wouldn't compile without this, though for now they remain unimplemented...
pub struct Array<'a> { _marker: std::marker::PhantomData<&'a ()> }
pub struct Object<'a> { _marker: std::marker::PhantomData<&'a ()> }
pub struct AbstractType<'a> { _marker: std::marker::PhantomData<&'a ()> }
pub struct FunctionType<'a> { _marker: std::marker::PhantomData<&'a ()> }
pub struct Script<'a> { _marker: std::marker::PhantomData<&'a ()> }
pub struct Closure<'a> { _marker: std::marker::PhantomData<&'a ()> }
pub struct Instance<'a> { _marker: std::marker::PhantomData<&'a ()> }
pub struct WeakArray<'a> { _marker: std::marker::PhantomData<&'a ()> }
pub struct TypedDataBase<'a> { _marker: std::marker::PhantomData<&'a ()> }
pub struct TypedData<'a> { _marker: std::marker::PhantomData<&'a ()> }
pub struct TypedDataView<'a> { _marker: std::marker::PhantomData<&'a ()> }
pub struct GrowableObjectArray<'a> { _marker: std::marker::PhantomData<&'a ()> }
pub struct Code<'a> { _marker: std::marker::PhantomData<&'a ()> }
pub struct LoadingUnit<'a> { _marker: std::marker::PhantomData<&'a ()> }
