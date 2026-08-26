use crate::stream::Stream;

macro_rules! object_store_aot_fields {
    ($emit:ident) => {
        $emit! {
                // Layout for the 3.11.X Dart version, and as early as 3.11.0
                list_class: u32,                                           // ClassPtr
                map_class: u32,                                            // ClassPtr
                set_class: u32,                                            // ClassPtr
                non_nullable_list_rare_type: u32,                          // TypePtr
                non_nullable_map_rare_type: u32,                           // TypePtr
                enum_index_field: u32,                                     // FieldPtr
                enum_name_field: u32,                                      // FieldPtr
                _object_equals_function: u32,                              // FunctionPtr
                _object_hash_code_function: u32,                           // FunctionPtr
                _object_to_string_function: u32,                           // FunctionPtr
                symbol_class: u32,                                         // ClassPtr
                symbol_name_field: u32,                                    // FieldPtr
                ffi_array_class: u32,                                      // ClassPtr
                ffi_compound_class: u32,                                   // ClassPtr
                ffi_struct_class: u32,                                     // ClassPtr
                ffi_union_class: u32,                                      // ClassPtr
                ffi_varargs_class: u32,                                    // ClassPtr
                compound_offset_in_bytes_field: u32,                       // FieldPtr
                compound_typed_data_base_field: u32,                       // FieldPtr
                ffi_resolver_function: u32,                                // FunctionPtr
                handle_finalizer_message_function: u32,                    // FunctionPtr
                handle_native_finalizer_message_function: u32,             // FunctionPtr
                non_nullable_future_never_type: u32,                       // TypePtr
                nullable_future_null_type: u32,                            // TypePtr
                send_port_class: u32,                                      // ClassPtr
                capability_class: u32,                                     // ClassPtr
                transferable_class: u32,                                   // ClassPtr
                lookup_port_handler: u32,                                  // FunctionPtr
                lookup_open_ports: u32,                                    // FunctionPtr
                handle_message_function: u32,                              // FunctionPtr
                object_class: u32,                                         // ClassPtr
                object_type: u32,                                          // TypePtr
                non_nullable_object_type: u32,                             // TypePtr
                nullable_object_type: u32,                                 // TypePtr
                null_class: u32,                                           // ClassPtr
                null_type: u32,                                            // TypePtr
                never_class: u32,                                          // ClassPtr
                never_type: u32,                                           // TypePtr
                function_type: u32,                                        // TypePtr
                type_type: u32,                                            // TypePtr
                closure_class: u32,                                        // ClassPtr
                record_class: u32,                                         // ClassPtr
                number_type: u32,                                          // TypePtr
                nullable_number_type: u32,                                 // TypePtr
                int_type: u32,                                             // TypePtr
                non_nullable_int_type: u32,                                // TypePtr
                nullable_int_type: u32,                                    // TypePtr
                integer_implementation_class: u32,                         // ClassPtr
                int64_type: u32,                                           // TypePtr
                smi_class: u32,                                            // ClassPtr
                smi_type: u32,                                             // TypePtr
                mint_class: u32,                                           // ClassPtr
                mint_type: u32,                                            // TypePtr
                double_class: u32,                                         // ClassPtr
                double_type: u32,                                          // TypePtr
                nullable_double_type: u32,                                 // TypePtr
                float32x4_type: u32,                                       // TypePtr
                int32x4_type: u32,                                         // TypePtr
                float64x2_type: u32,                                       // TypePtr
                string_type: u32,                                          // TypePtr
                type_argument_int: u32,                                    // TypeArgumentsPtr
                type_argument_double: u32,                                 // TypeArgumentsPtr
                type_argument_never: u32,                                  // TypeArgumentsPtr
                type_argument_string: u32,                                 // TypeArgumentsPtr
                type_argument_string_dynamic: u32,                         // TypeArgumentsPtr
                type_argument_string_string: u32,                          // TypeArgumentsPtr
                compiletime_error_class: u32,                              // ClassPtr
                pragma_class: u32,                                         // ClassPtr
                pragma_name: u32,                                          // FieldPtr
                pragma_options: u32,                                       // FieldPtr
                future_class: u32,                                         // ClassPtr
                future_or_class: u32,                                      // ClassPtr
                one_byte_string_class: u32,                                // ClassPtr
                two_byte_string_class: u32,                                // ClassPtr
                bool_type: u32,                                            // TypePtr
                bool_class: u32,                                           // ClassPtr
                array_class: u32,                                          // ClassPtr
                array_type: u32,                                           // TypePtr
                immutable_array_class: u32,                                // ClassPtr
                growable_object_array_class: u32,                          // ClassPtr
                map_impl_class: u32,                                       // ClassPtr
                const_map_impl_class: u32,                                 // ClassPtr
                set_impl_class: u32,                                       // ClassPtr
                const_set_impl_class: u32,                                 // ClassPtr
                float32x4_class: u32,                                      // ClassPtr
                int32x4_class: u32,                                        // ClassPtr
                float64x2_class: u32,                                      // ClassPtr
                error_class: u32,                                          // ClassPtr
                expando_class: u32,                                        // ClassPtr
                iterable_class: u32,                                       // ClassPtr
                weak_property_class: u32,                                  // ClassPtr
                weak_reference_class: u32,                                 // ClassPtr
                finalizer_class: u32,                                      // ClassPtr
                finalizer_entry_class: u32,                                // ClassPtr
                native_finalizer_class: u32,                               // ClassPtr
                dart_condition_variable_class: u32,                        // ClassPtr
                dart_mutex_class: u32,                                     // ClassPtr
                symbol_table: u32,                                         // WeakArrayPtr
                regexp_table: u32,                                         // WeakArrayPtr
                canonical_types: u32,                                      // ArrayPtr
                canonical_function_types: u32,                             // ArrayPtr
                canonical_record_types: u32,                               // ArrayPtr
                canonical_type_parameters: u32,                            // ArrayPtr
                canonical_type_arguments: u32,                             // ArrayPtr
                async_library: u32,                                        // LibraryPtr
                core_library: u32,                                         // LibraryPtr
                _compact_hash_library: u32,                                // LibraryPtr
                collection_library: u32,                                   // LibraryPtr
                concurrent_library: u32,                                   // LibraryPtr
                convert_library: u32,                                      // LibraryPtr
                developer_library: u32,                                    // LibraryPtr
                ffi_library: u32,                                          // LibraryPtr
                _internal_library: u32,                                    // LibraryPtr
                isolate_library: u32,                                      // LibraryPtr
                math_library: u32,                                         // LibraryPtr
                mirrors_library: u32,                                      // LibraryPtr
                native_wrappers_library: u32,                              // LibraryPtr
                root_library: u32,                                         // LibraryPtr
                typed_data_library: u32,                                   // LibraryPtr
                _vm_library: u32,                                          // LibraryPtr
                _vmservice_library: u32,                                   // LibraryPtr
                native_assets_library: u32,                                // LibraryPtr
                native_assets_map: u32,                                    // ArrayPtr
                libraries: u32,                                            // GrowableObjectArrayPtr
                libraries_map: u32,                                        // ArrayPtr
                uri_to_resolved_uri_map: u32,                              // ArrayPtr
                resolved_uri_to_uri_map: u32,                              // ArrayPtr
                last_libraries_count: u32,                                 // SmiPtr
                loading_units: u32,                                        // ArrayPtr
                closure_functions: u32,                                    // GrowableObjectArrayPtr
                closure_functions_table: u32,                              // ArrayPtr
                pending_classes: u32,                                      // GrowableObjectArrayPtr
                record_field_names_map: u32,                               // ArrayPtr
                record_field_names: u32,                                   // ArrayPtr
                stack_overflow: u32,                                       // InstancePtr
                out_of_memory: u32,                                        // InstancePtr
                growable_list_factory: u32,                                // FunctionPtr
                simple_instance_of_function: u32,                          // FunctionPtr
                simple_instance_of_true_function: u32,                     // FunctionPtr
                simple_instance_of_false_function: u32,                    // FunctionPtr
                async_star_stream_controller_add: u32,                     // FunctionPtr
                async_star_stream_controller_add_stream: u32,              // FunctionPtr
                suspend_state_init_async: u32,                             // FunctionPtr
                suspend_state_await: u32,                                  // FunctionPtr
                suspend_state_await_with_type_check: u32,                  // FunctionPtr
                suspend_state_return_async: u32,                           // FunctionPtr
                suspend_state_return_async_not_future: u32,                // FunctionPtr
                suspend_state_init_async_star: u32,                        // FunctionPtr
                suspend_state_yield_async_star: u32,                       // FunctionPtr
                suspend_state_return_async_star: u32,                      // FunctionPtr
                suspend_state_init_sync_star: u32,                         // FunctionPtr
                suspend_state_suspend_sync_star_at_start: u32,             // FunctionPtr
                suspend_state_handle_exception: u32,                       // FunctionPtr
                async_star_stream_controller: u32,                         // ClassPtr
                stream_class: u32,                                         // ClassPtr
                sync_star_iterator_class: u32,                             // ClassPtr
                async_star_stream_controller_async_star_body: u32,         // FieldPtr
                sync_star_iterator_current: u32,                           // FieldPtr
                sync_star_iterator_state: u32,                             // FieldPtr
                sync_star_iterator_yield_star_iterable: u32,               // FieldPtr
                canonicalized_stack_map_entries: u32,                      // CompressedStackMapsPtr
                global_object_pool: u32,                                   // ObjectPoolPtr
                unique_dynamic_targets: u32,                               // ArrayPtr
                megamorphic_cache_table: u32,                              // GrowableObjectArrayPtr
                ffi_callback_code: u32,                                    // GrowableObjectArrayPtr
                dispatch_table_null_error_stub: u32,                       // CodePtr
                late_initialization_error_stub_with_fpu_regs_stub: u32,    // CodePtr
                late_initialization_error_stub_without_fpu_regs_stub: u32, // CodePtr
                null_error_stub_with_fpu_regs_stub: u32,                   // CodePtr
                null_error_stub_without_fpu_regs_stub: u32,                // CodePtr
                null_arg_error_stub_with_fpu_regs_stub: u32,               // CodePtr
                null_arg_error_stub_without_fpu_regs_stub: u32,            // CodePtr
                null_cast_error_stub_with_fpu_regs_stub: u32,              // CodePtr
                null_cast_error_stub_without_fpu_regs_stub: u32,           // CodePtr
                range_error_stub_with_fpu_regs_stub: u32,                  // CodePtr
                range_error_stub_without_fpu_regs_stub: u32,               // CodePtr
                write_error_stub_with_fpu_regs_stub: u32,                  // CodePtr
                write_error_stub_without_fpu_regs_stub: u32,               // CodePtr
                field_access_error_stub_with_fpu_regs_stub: u32,           // CodePtr
                field_access_error_stub_without_fpu_regs_stub: u32,        // CodePtr
                allocate_mint_with_fpu_regs_stub: u32,                     // CodePtr
                allocate_mint_without_fpu_regs_stub: u32,                  // CodePtr
                stack_overflow_stub_with_fpu_regs_stub: u32,               // CodePtr
                stack_overflow_stub_without_fpu_regs_stub: u32,            // CodePtr
                allocate_array_stub: u32,                                  // CodePtr
                allocate_mint_stub: u32,                                   // CodePtr
                allocate_double_stub: u32,                                 // CodePtr
                allocate_float32x4_stub: u32,                              // CodePtr
                allocate_float64x2_stub: u32,                              // CodePtr
                allocate_int32x4_stub: u32,                                // CodePtr
                allocate_int8_array_stub: u32,                             // CodePtr
                allocate_uint8_array_stub: u32,                            // CodePtr
                allocate_uint8_clamped_array_stub: u32,                    // CodePtr
                allocate_int16_array_stub: u32,                            // CodePtr
                allocate_uint16_array_stub: u32,                           // CodePtr
                allocate_int32_array_stub: u32,                            // CodePtr
                allocate_uint32_array_stub: u32,                           // CodePtr
                allocate_int64_array_stub: u32,                            // CodePtr
                allocate_uint64_array_stub: u32,                           // CodePtr
                allocate_float32_array_stub: u32,                          // CodePtr
                allocate_float64_array_stub: u32,                          // CodePtr
                allocate_float32x4_array_stub: u32,                        // CodePtr
                allocate_int32x4_array_stub: u32,                          // CodePtr
                allocate_float64x2_array_stub: u32,                        // CodePtr
                allocate_closure_stub: u32,                                // CodePtr
                allocate_closure_generic_stub: u32,                        // CodePtr
                allocate_closure_ta_stub: u32,                             // CodePtr
                allocate_closure_ta_generic_stub: u32,                     // CodePtr
                allocate_context_stub: u32,                                // CodePtr
                allocate_growable_array_stub: u32,                         // CodePtr
                allocate_object_stub: u32,                                 // CodePtr
                allocate_object_parametrized_stub: u32,                    // CodePtr
                allocate_record_stub: u32,                                 // CodePtr
                allocate_record2_stub: u32,                                // CodePtr
                allocate_record2_named_stub: u32,                          // CodePtr
                allocate_record3_stub: u32,                                // CodePtr
                allocate_record3_named_stub: u32,                          // CodePtr
                allocate_unhandled_exception_stub: u32,                    // CodePtr
                check_isolate_field_access_stub: u32,                      // CodePtr
                clone_context_stub: u32,                                   // CodePtr
                write_barrier_wrappers_stub: u32,                          // CodePtr
                array_write_barrier_stub: u32,                             // CodePtr
                throw_stub: u32,                                           // CodePtr
                re_throw_stub: u32,                                        // CodePtr
                instance_of_stub: u32,                                     // CodePtr
                init_static_field_stub: u32,                               // CodePtr
                init_late_static_field_stub: u32,                          // CodePtr
                init_late_final_static_field_stub: u32,                    // CodePtr
                init_instance_field_stub: u32,                             // CodePtr
                init_late_instance_field_stub: u32,                        // CodePtr
                init_late_final_instance_field_stub: u32,                  // CodePtr
                init_shared_late_static_field_stub: u32,                   // CodePtr
                call_closure_no_such_method_stub: u32,                     // CodePtr
                default_tts_stub: u32,                                     // CodePtr
                default_nullable_tts_stub: u32,                            // CodePtr
                top_type_tts_stub: u32,                                    // CodePtr
                nullable_type_parameter_tts_stub: u32,                     // CodePtr
                type_parameter_tts_stub: u32,                              // CodePtr
                unreachable_tts_stub: u32,                                 // CodePtr
                ffi_callback_functions: u32,                               // ArrayPtr
                resume_stub: u32,                                          // CodePtr
                slow_tts_stub: u32,                                        // CodePtr (last field in FullAOT)
        }
    };
}

macro_rules! define_object_store {
    ($( $field:ident: u32,)*) => {
        #[derive(Default)]
        #[repr(C)]
        pub(crate) struct ObjectStore {
            $( $field: u32, )*
        }

        impl ObjectStore {
            pub const REF_COUNT: usize = [$(stringify!($field)),*].len(); // this is probably better than sizeof(ObjectStore)/sizeof(u32)

            pub fn read(stream: &mut Stream) -> anyhow::Result<Self> {
                Ok(Self {
                    $( $field: stream.read_ref_id()?, )*
                })
            }

            pub fn global_object_pool(&self) -> u32 {
                self.global_object_pool
            }
        }
    };
}

object_store_aot_fields!(define_object_store);

pub(super) struct IsolateObjectStore
// small one, not too important, skip for now
{
    dart_args_1: u32,         // ArrayPtr
    dart_args_2: u32,         // ArrayPtr
    resume_capabilities: u32, // GrowableObjectArrayPtr
    exit_listeners: u32,      // GrowableObjectArrayPtr
    error_listeners: u32,     // GrowableObjectArrayPtr
}
#[derive(Default)]
pub(crate) struct FieldTable {
    pub(super) length: usize,
    pub(super) field_refs: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DispatchTableEntry {
    Invalid,
    CodeIndex(u64), // this is an index INTO the instruction table
}

#[derive(Debug, Default)]
pub(crate) struct DispatchTable {
    pub(super) first_code_ref: Option<u32>,
    pub(super) entries: Vec<DispatchTableEntry>,
}

#[derive(Default)]
pub struct ProgramRoots {
    object_store: ObjectStore,
    field_table: FieldTable,
    shared_field_table: FieldTable,
    dispatch_table: DispatchTable,
}

impl ProgramRoots {
    pub(crate) fn new(
        object_store: ObjectStore,
        field_table: FieldTable,
        shared_field_table: FieldTable,
        dispatch_table: DispatchTable,
    ) -> Self {
        Self {
            object_store,
            field_table,
            shared_field_table,
            dispatch_table,
        }
    }

    pub(crate) fn object_store(&self) -> &ObjectStore
    {
        &self.object_store
    }
}
