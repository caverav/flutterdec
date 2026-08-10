pub type Smi = i32;

#[derive(Default)]
pub struct Object {
    pub tags: u64,
}

#[derive(Default)]
pub struct Class {
    pub id: i32,
    pub is_predefined: bool,
    pub name: u32,                                // StringPtr
    pub user_name: u32,                           // StringPtr
    pub functions: u32,                           // ArrayPtr
    pub functions_hash_table: u32,                // ArrayPtr
    pub fields: u32,                              // ArrayPtr
    pub offset_in_words_to_field: u32,            // ArrayPtr
    pub interfaces: u32,                          // ArrayPtr
    pub script: u32,                              // ScriptPtr
    pub library: u32,                             // LibraryPtr
    pub type_parameters: u32,                     // TypeParametersPtr
    pub super_type: u32,                          // TypePtr
    pub constants: u32,                           // ArrayPtr
    pub declaration_type: u32,                    // TypePtr
    pub invocation_dispatcher_cache: u32,         // ArrayPtr
    pub direct_implementors: u32,                 // GrowableObjectArrayPtr
    pub direct_subclasses: u32,                   // GrowableObjectArrayPtr
    pub declaration_instance_type_arguments: u32, // TypeArgumentsPtr
    pub allocation_stub: u32,                     // CodePtr
    pub dependent_code: u32,                      // WeakArrayPtr
    pub num_native_fields: u16,
    pub state_bits: u32,
    pub kernel_offset: u32,
    pub num_type_arguments: i16,
    pub host_instance_size_in_words: i32,
    pub host_type_arguments_field_offset_in_words: i32,
    pub host_next_field_offset_in_words: i32,
    pub target_instance_size_in_words: i32,
    pub target_type_arguments_field_offset_in_words: i32,
    pub target_next_field_offset_in_words: i32,
    pub unboxed_fields_bitmap: Option<u64>,
}

#[derive(Default)]
pub struct PatchClass {
    pub wrapped_class: u32,       // ClassPtr
    pub script: u32,              // ScriptPtr
    pub kernel_program_info: u32, // KernelProgramInfoPtr
}

#[derive(Default)]
pub struct Function {
    pub name: u32,      // StringPtr
    pub owner: u32,     // ObjectPtr
    pub signature: u32, // FunctionTypePtr
    pub data: u32,      // ObjectPtr
    // pub ic_data_array_or_bytecode: u32,  // ObjectPtr [[NOT PRESENT IN FullAOT]]
    pub code_index: u32, // unsigned integer index
    // pub positional_parameter_names: u32, // ArrayPtr [[NOT PRESENT IN FullAOT]]
    // pub unoptimized_code: u32,           // CodePtr [[NOT PRESENT IN FullAOT]]
    // pub bitmap: u64,                     [[NOT PRESENT IN FullAOT]]
    pub token_pos: i32,
    // pub kernel_offset: u32,              [[NOT PRESENT IN FullAOT]]
    pub kind_tag: u32,
}

#[derive(Default)]
pub struct ClosureData {
    pub context_scope: u32,   // ContextScopePtr
    pub parent_function: u32, // FunctionPtr
    pub closure: u32,         // ClosurePtr
    pub packed_fields: u32,
}

#[derive(Default)]
pub struct FfiTrampolineData {
    pub signature_type: u32,              // TypePtr
    pub c_signature: u32,                 // FunctionTypePtr
    pub callback_target: u32,             // FunctionPtr
    pub callback_exceptional_return: u32, // InstancePtr
    pub ffi_function_kind: u8,
    pub callback_id: i32,
}

#[derive(Default)]
pub struct Field {
    pub name: u32,                    // StringPtr
    pub owner: u32,                   // ObjectPtr
    pub type_field: u32,              // AbstractTypePtr
    pub initializer_function: u32,    // FunctionPtr
    pub host_offset_or_field_id: u32, // SmiPtr
    // pub guarded_list_length: u32,     // SmiPtr [[NOT PRESENT IN FullAOT]]
    // pub exact_type: u32,              // AbstractTypePtr [[NOT PRESENT IN FullAOT]]
    // pub dependent_code: u32,          // WeakArrayPtr [[NOT PRESENT IN FullAOT]]
    pub token_pos: i32,
    pub end_token_pos: i32,
    pub guarded_cid: u32,
    pub is_nullable: u32,
    // pub kernel_offset: u32,           [[NOT PRESENT IN FullAOT]]
    // pub guarded_list_length_in_object_offset: i8, [[NOT PRESENT IN FullAOT]]
    // pub static_type_exactness_state: i8, [[NOT PRESENT IN FullAOT]]
    // pub target_offset: i32,           [[NOT PRESENT IN FullAOT]]
    pub kind_bits: u32,
}

#[derive(Default)]
pub struct Script {
    // Fieldless class
}

#[derive(Default)]
pub struct Library {
    pub name: u32,           // StringPtr
    pub url: u32,            // StringPtr
    pub private_key: u32,    // StringPtr
    pub dictionary: u32,     // ArrayPtr
    pub metadata: u32,       // ArrayPtr
    pub toplevel_class: u32, // ClassPtr
    pub used_scripts: u32,   // GrowableObjectArrayPtr
    pub loading_unit: u32,   // LoadingUnitPtr
    pub imports: u32,        // ArrayPtr
    pub exports: u32,        // ArrayPtr
    // pub dependencies: u32,        // ArrayPtr [[NOT PRESENT IN FullAOT]]
    // pub kernel_program_info: u32, // KernelProgramInfoPtr [[NOT PRESENT IN FullAOT]]
    // pub loaded_scripts: u32,      // ArrayPtr [[NOT PRESENT IN FullAOT]]
    pub index: i32,
    pub num_imports: u16,
    pub load_state: i8,
    pub flags: u8,
    // pub kernel_library_index: u32, [[NOT PRESENT IN FullAOT]]
}

#[derive(Default)]
pub struct Namespace {
    pub target: u32,     // LibraryPtr
    pub show_names: u32, // ArrayPtr
    pub hide_names: u32, // ArrayPtr
    pub owner: u32,      // LibraryPtr
}

#[derive(Default)]
pub struct KernelProgramInfo {
    pub kernel_component: u32,  // TypedDataBasePtr
    pub string_offsets: u32,    // TypedDataPtr
    pub string_data: u32,       // TypedDataViewPtr
    pub canonical_names: u32,   // TypedDataPtr
    pub metadata_payloads: u32, // TypedDataViewPtr
    pub metadata_mappings: u32, // TypedDataViewPtr
    pub scripts: u32,           // ArrayPtr
    pub constants: u32,         // ArrayPtr
    pub constants_table: u32,   // TypedDataViewPtr
    pub libraries_cache: u32,   // ArrayPtr
    pub classes_cache: u32,     // ArrayPtr
}

#[derive(Default)]
pub struct CodeSourceMap {
    pub length: u32,
    pub data: Vec<u8>,
}

#[derive(Default)]
pub struct CompressedStackMaps {
    pub length: u32,
    pub flags_and_size: u32,
    pub data: Vec<u8>,
}

#[derive(Default)]
pub struct PcDescriptors {
    // not really
    pub length: u32,
    pub data: Vec<u8>,
}

#[derive(Default)]
pub struct ExceptionHandlers {
    pub handled_types_data: u32, // ArrayPtr
    pub packed_fields: u32,
    pub num_entries: usize,
    pub entries: Vec<ExceptionHandlerInfo>,
}

#[derive(Default)]
pub struct ExceptionHandlerInfo {
    pub handler_pc_offset: u32,
    pub outer_try_index: i16,
    pub needs_stacktrace: i8,
    pub has_catch_all: i8,
    pub is_generated: i8,
}

#[derive(Default)]
pub struct Context {
    pub parent: u32, // ContextPtr
    pub num_variables: i32,
    pub variables: Vec<u32>,
}

#[derive(Default)]
pub struct ContextScope {
    pub num_variables: i32,
    pub is_implicit: bool,
    /// Flattened `VariableDesc` reference fields. There are ten per variable
    /// in this SDK revision.
    pub variables: Vec<u32>,
}

#[derive(Default)]
pub struct UnlinkedCall {
    pub can_patch_to_monomorphic: bool,
}

#[derive(Default)]
pub struct ObjectPool {
    data: Vec<u32>, // vector holding the array of reference ids making up the object pool
}

#[derive(Default)]
pub struct Mint {
    pub value: i64, // ALIGN8
}

#[derive(Default)]
pub struct Double {
    pub value: f64, // ALIGN8
}

#[derive(Default)]
pub struct TypeArguments {
    pub instantiations: u32, // ArrayPtr
    pub length: Smi,         // SmiPtr
    pub hash: Smi,           // SmiPtr
    pub nullability: Smi,    // SmiPtr
    pub types: Vec<u32>,     // AbstractTypePtr elements
}

#[derive(Default)]
pub struct TypeParameter {
    pub type_test_stub: u32, // CodePtr
    pub hash: u32,           // SmiPtr
    pub owner: u32,          // ObjectPtr
    pub base: u16,
    pub index: u16,
    pub flags: u8,
}

#[derive(Default)]
pub struct Type {
    pub type_test_stub: u32, // CodePtr
    pub hash: u32,           // SmiPtr
    pub arguments: u32,      // TypeArgumentsPtr
    pub flags: u8,
}

#[derive(Default)]
pub struct TypeParameters {
    pub names: u32,    // ArrayPtr
    pub flags: u32,    // ArrayPtr
    pub bounds: u32,   // TypeArgumentsPtr
    pub defaults: u32, // TypeArgumentsPtr
}

/*
    No need to make two separate structs here. Better to just
    have the _String class and add an enum field to determine
    the number of bytes "StrType".

#[derive(Default)]
pub struct OneByteString {
    // Fieldless class
}

#[derive(Default)]
pub struct TwoByteString {
    // Fieldless class
}
*/

#[derive(Default)]
pub enum StrType {
    #[default]
    OneByte, // assume the string is a one byte string.
    TwoByte,
}

#[derive(Default)]
pub struct _String {
    pub string_type: StrType,
    pub hash: Smi,   // SmiPtr
    pub length: Smi, // SmiPtr
    pub internal_str: String,
}

#[derive(Default)]
pub struct Array {
    pub type_arguments: u32, // TypeArgumentsPtr
    pub length: Smi,         // SmiPtr
    pub elements: Vec<u32>,  // ObjectPtr elements
}

#[derive(Default)]
pub struct AbstractType {
    pub type_test_stub: u32, // CodePtr
    pub hash: u32,           // SmiPtr
    pub padding: u32,
    pub flags: u32,
}

#[derive(Default)]
pub struct FunctionType {
    pub type_test_stub: u32,        // CodePtr
    pub hash: u32,                  // SmiPtr
    pub type_parameters: u32,       // TypeParametersPtr
    pub result_type: u32,           // AbstractTypePtr
    pub parameter_types: u32,       // ArrayPtr
    pub named_parameter_names: u32, // ArrayPtr
    pub flags: u8,
    pub packed_parameter_counts: u32,
    pub packed_type_parameter_counts: u16,
}

#[derive(Default)]
pub struct Closure {
    pub instantiator_type_arguments: u32, // TypeArgumentsPtr
    pub function_type_arguments: u32,     // TypeArgumentsPtr
    pub delayed_type_arguments: u32,      // TypeArgumentsPtr
    pub function: u32,                    // FunctionPtr
    pub context: u32,                     // ObjectPtr
    pub hash: u32,                        // SmiPtr
}

#[derive(Default)]
pub struct Instance {
    pub next_field_offset_in_words: i32,
    pub instance_size_in_words: i32,
    pub unboxed_fields_bitmap: u64,
    pub fields: Vec<InstanceField>,
}

#[derive(Debug)]
pub enum InstanceField {
    Reference(u32),
    Unboxed(u64),
}

#[derive(Default)]
pub struct WeakArray {
    pub next_seen_by_gc: u32, // WeakArrayPtr
    pub length: Smi,          // SmiPtr
    pub elements: Vec<u32>,   // ObjectPtr elements
}

#[derive(Default)]
pub struct TypedDataBase {
    pub length: Smi, // SmiPtr
    pub padding: u32,
}

#[derive(Default)]
pub struct TypedData {
    pub length: usize,
    pub data: Vec<u8>,
}

#[derive(Default)]
pub struct TypedDataView {
    pub typed_data: u32,      // TypedDataBasePtr
    pub offset_in_bytes: Smi, // SmiPtr
}

#[derive(Default)]
pub struct GrowableObjectArray {
    pub type_arguments: u32, // TypeArgumentsPtr
    pub data: u32,           // ArrayPtr
    pub length: Smi,         // SmiPtr
}

#[derive(Default)]
pub struct Code {
    // these four are NOT computed during our cluster deserialization as we need the information
    // in the instructions table in order to do so, and this table is
    // read after the clustered stream reading, which is waht the function resolve_entrypoints does
    pub entry_point: u64,             // unset before resolve_entrypoints
    pub monomorphic_entry_point: u64, // unset before resolve_entrypoints
    pub unchecked_entry_point: u64,   // set to unchecked_offset before resolve_entrypoints
    pub monomorphic_unchecked_entry_point: u64, // set to unchecked_offset before resolve_entrypoints

    pub has_monomorphic_entrypoint: bool, // this field doesn't exist in UntaggedCode, its here so
    // resolve_entrypoints can make use of it
    pub object_pool: u32,          // ObjectPoolPtr
    pub instructions: u32,         // InstructionsPtr
    pub owner: u32, // ClassPtr or FunctionPtr or null, but the actual type in the class is an ObjectPtr
    pub exception_handlers: u32, // ExceptionHandlerPtr
    pub pc_descriptors: u32, // PcDescriptorsPtr
    pub catch_entry: u32, // ObjectPtr
    pub compressed_stackmaps: u32, // CompressedStackMapsPtr
    pub inlined_id_to_function: u32,
    pub code_source_map: u32, // CodeSourceMapPtr

    // pub active_instructions: u32, // InstructionsPtr [[NOT PRESENT IN FullAOT]]
    // pub deopt_info_array: u32, // ArrayPtr [[NOT PRESENT IN FullAOT]]
    // pub static_calls_target_table: u32, // ArrayPtr [[NOT PRESENT IN FullAOT]]
    pub state_bits: i32,
    pub instructions_length_: u32,
}

#[derive(Default)]
pub struct LoadingUnit {
    pub parent: u32,       // LoadingUnitPtr
    pub base_objects: u32, // ArrayPtr
    pub packed_fields: i64,
}

#[derive(Default)]
pub struct ICData {
    pub target_name: u32,
    pub args_descriptor: u32,
    pub entries: u32,
    pub state_bits: u32,
}

#[derive(Default)]
pub struct MegamorphicCache {
    pub target_name: u32,
    pub args_descriptor: u32,
    pub buckets: u32,
    pub mask: Smi,
    pub filled_entry_count: i32,
}

#[derive(Default)]
pub struct SubtypeTestCache {
    pub cache: u32,
    pub num_inputs: u32,
    pub num_occupied: u32,
}

#[derive(Default)]
pub struct LanguageError {
    pub previous_error: u32,    // ErrorPtr
    pub script: u32,            // ScriptPtr
    pub message: u32,           // StringPtr
    pub formatted_message: u32, // StringPtr
    pub token_pos: i32,
    pub report_after_token: bool,
    pub kind: i8,
}

#[derive(Default)]
pub struct UnhandledException {
    pub exception: u32,  // InstancePtr
    pub stacktrace: u32, // InstancePtr
}

#[derive(Default)]
pub struct LibraryPrefix {
    pub name: u32,     // StringPtr
    pub imports: u32,  // ArrayPtr
    pub importer: u32, // LibraryPtr
    pub num_imports: u16,
    pub is_deferred_load: bool,
}

#[derive(Default)]
pub struct RecordType {
    pub type_test_stub: u32, // CodePtr
    pub hash: u32,           // SmiPtr
    pub shape: Smi,          // SmiPtr
    pub field_types: u32,    // ArrayPtr
    pub flags: u8,
}

#[derive(Default)]
pub struct Int32x4 {
    // Fieldless class
}

#[derive(Default)]
pub struct ExternalTypedData {
    // Fieldless class
}

#[derive(Default)]
pub struct StackTrace {
    pub async_link: u32, // StackTracePtr
    pub code_array: u32, // ArrayPtr
    pub pc_offset_array: u32, // TypedDataPtr
                         // pub expand_inlined: bool, [[NOT PRESENT IN FullAOT]]
}

#[derive(Default)]
pub struct RegExp {
    pub capture_name_map: u32, // ArrayPtr
    pub pattern: u32,          // StringPtr
    pub one_byte: u32,         // TypedDataPtr
    pub two_byte: u32,         // TypedDataPtr
    pub one_byte_sticky: u32,  // TypedDataPtr
    pub two_byte_sticky: u32,  // TypedDataPtr
    pub num_one_byte_registers: i32,
    pub num_two_byte_registers: i32,
    pub type_flags: i8,
}

#[derive(Default)]
pub struct WeakProperty {
    pub key: u32, // ObjectPtr
    pub value: u32, // ObjectPtr
                  // pub next_seen_by_gc: u32, // WeakPropertyPtr [[NOT PRESENT IN FullAOT]]
}

#[derive(Default)]
pub struct Map {
    pub type_arguments: u32,
    pub hash_mask: u32,
    pub data: u32,
    pub used_data: u32,
    pub deleted_keys: u32,
    pub index: u32,
}

#[derive(Default)]
pub struct Set {
    pub type_arguments: u32,
    pub hash_mask: u32,
    pub data: u32,
    pub used_data: u32,
    pub deleted_keys: u32,
    pub index: u32,
}

#[derive(Default)]
pub struct Float32x4 {
    pub value: Vec<u8>,
}

#[derive(Default)]
pub struct Float64x2 {
    pub value: Vec<u8>,
}

#[derive(Default)]
pub struct ConstMap {
    pub type_arguments: u32,
    pub hash_mask: u32,
    pub data: u32,
    pub used_data: u32,
    pub deleted_keys: u32,
    pub index: u32,
}

#[derive(Default)]
pub struct ConstSet {
    pub type_arguments: u32,
    pub hash_mask: u32,
    pub data: u32,
    pub used_data: u32,
    pub deleted_keys: u32,
    pub index: u32,
}

#[derive(Default)]
pub struct Record {
    pub shape: Smi, // SmiPtr
    pub padding: u32,
    pub num_fields: usize,
    pub fields: Vec<u32>,
}

#[derive(Default)]
pub struct ImmutableArray {
    pub type_arguments: u32,
    pub length: Smi,
    pub elements: Vec<u32>,
}
