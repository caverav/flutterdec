use std::env::var;
use std::usize;

use crate::constants::{ClassId, ClassId::*, SIGNED_M, UNSIGNED_M};
use crate::raw_object::*;
use crate::stream::Stream;
use crate::DECLARE_FIXED_LENGTH_CLUSTER;
use crate::DECLARE_VARIABLE_LENGTH_CLUSTER;
use crate::FFI_TYPES_LIST;

pub trait Cluster {
    fn is_fixed_len(&self) -> bool;
    fn read_alloc(&mut self, last_ref_id: &mut u64, stream: &mut Stream) -> anyhow::Result<usize>;
    fn read_fill(&mut self, stream: &mut Stream) -> anyhow::Result<usize>;
}

pub fn read_smi(stream: &mut Stream) -> anyhow::Result<Smi> {
    let raw_smi = stream.read()?; // smis are always written as signed numbers

    Ok(raw_smi as Smi)
}

macro_rules! FFI_CASE_PATTERN {
    ( $( $ffi_type:ident ),* ) => {
        $( $ffi_type )|*
    };
}

pub fn decide_cluster(class_id: ClassId) -> Result<Box<dyn Cluster>, &'static str> {
    match class_id {
        // we assume compressed pointers, it supports only Android for now...
        IllegalCid => Err("Not a supported class (illegal class)..."),
        FFI_TYPES_LIST!(FFI_CASE_PATTERN) => Err("To do..."),
        _ => Err("Not a supported class..."),
    }
}

// These are the objects that call ReadAllocFixedSize during deserialization,
// whose fill cluster size is uniquely determined by sizeof(Object) * num_of_objects
// and alloc cluster size is tags (MULEB128) + num_of_objects (MULEB128)

DECLARE_FIXED_LENGTH_CLUSTER!(TypeParameters, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.names = stream.read_ref_id()?;
        obj.flags = stream.read_ref_id()?;
        obj.bounds = stream.read_ref_id()?;
        obj.defaults = stream.read_ref_id()?;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(PatchClass, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.wrapped_class = stream.read_ref_id()?;
        obj.script = stream.read_ref_id()?;
        obj.kernel_program_info = stream.read_ref_id()?;
        // obj.kernel_library_index = stream.read_unsigned()? as u32; [[NOT PRESENT IN FullAOT]]
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Function, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.name = stream.read_ref_id()?;
        obj.owner = stream.read_ref_id()?;
        obj.signature = stream.read_ref_id()?;
        obj.data = stream.read_ref_id()?;
        // obj.ic_data_array_or_bytecode = stream.read_ref_id()?; [[NOT PRESENT IN FullAOT]]
        // obj.code = stream.read_ref_id()?; [[SKIPPED BY WriteFromTo]]
        // obj.positional_parameter_names = stream.read_ref_id()?; [[NOT PRESENT IN FullAOT]]
        // obj.unoptimized_code = stream.read_ref_id()?; [[NOT PRESENT IN FullAOT]]
        // obj.bitmap = stream.read_unsigned()? as u64; [[NOT PRESENT IN FullAOT]]
        obj.code_index = stream.read_unsigned()? as u32;
        obj.token_pos = stream.read()? as i32;
        // obj.kernel_offset = stream.read_unsigned()? as u32; [[NOT PRESENT IN FullAOT]]
        obj.kind_tag = stream.read_unsigned()? as u32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(ClosureData, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.context_scope = stream.read_ref_id()?;
        obj.parent_function = stream.read_ref_id()?;
        obj.closure = stream.read_ref_id()?;
        obj.packed_fields = stream.read_unsigned()? as u32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(FfiTrampolineData, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.signature_type = stream.read_ref_id()?;
        obj.c_signature = stream.read_ref_id()?;
        obj.callback_target = stream.read_ref_id()?;
        obj.callback_exceptional_return = stream.read_ref_id()?;
        obj.ffi_function_kind = stream.read_unsigned()? as u8;
        obj.callback_id = stream.read()? as i32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Field, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.name = stream.read_ref_id()?;
        obj.owner = stream.read_ref_id()?;
        obj.type_field = stream.read_ref_id()?;
        obj.initializer_function = stream.read_ref_id()?;
        obj.host_offset_or_field_id = stream.read_ref_id()?;
        // obj.guarded_list_length = stream.read_ref_id()?; [[NOT PRESENT IN FullAOT]]
        // obj.exact_type = stream.read_ref_id()?; [[NOT PRESENT IN FullAOT]]
        // obj.dependent_code = stream.read_ref_id()?; [[NOT PRESENT IN FullAOT]]
        obj.token_pos = stream.read()? as i32;
        obj.end_token_pos = stream.read()? as i32;
        obj.guarded_cid = stream.read_unsigned()? as u32;
        obj.is_nullable = stream.read_unsigned()? as u32;
        // obj.kernel_offset = stream.read_unsigned()? as u32; [[NOT PRESENT IN FullAOT]]
        // obj.guarded_list_length_in_object_offset = stream.read()? as i8; [[NOT PRESENT IN FullAOT]]
        // obj.static_type_exactness_state = stream.read()? as i8; [[NOT PRESENT IN FullAOT]]
        // obj.target_offset = stream.read()? as i32; [[NOT PRESENT IN FullAOT]]
        obj.kind_bits = stream.read_unsigned()? as u32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Script, |_self, stream| {
    for _ in 0.._self.obj_count as usize {}
});
DECLARE_FIXED_LENGTH_CLUSTER!(Library, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.name = stream.read_ref_id()?;
        obj.url = stream.read_ref_id()?;
        obj.private_key = stream.read_ref_id()?;
        obj.dictionary = stream.read_ref_id()?;
        obj.metadata = stream.read_ref_id()?;
        obj.toplevel_class = stream.read_ref_id()?;
        obj.used_scripts = stream.read_ref_id()?;
        obj.loading_unit = stream.read_ref_id()?;
        obj.imports = stream.read_ref_id()?;
        obj.exports = stream.read_ref_id()?;
        // obj.dependencies = stream.read_ref_id()?; [[NOT PRESENT IN FullAOT]]
        // obj.kernel_program_info = stream.read_ref_id()?; [[NOT PRESENT IN FullAOT]]
        // obj.loaded_scripts = stream.read_ref_id()?; [[NOT PRESENT IN FullAOT]]
        obj.index = stream.read()? as i32;
        obj.num_imports = stream.read()? as u16;
        obj.load_state = stream.read()? as i8;
        obj.flags = stream.read()? as u8;
        // obj.kernel_library_index = stream.read_unsigned()? as u32; [[NOT PRESENT IN FullAOT]]
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Namespace, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.target = stream.read_ref_id()?;
        obj.show_names = stream.read_ref_id()?;
        obj.hide_names = stream.read_ref_id()?;
        obj.owner = stream.read_ref_id()?;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(KernelProgramInfo, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.kernel_component = stream.read_ref_id()?;
        obj.string_offsets = stream.read_ref_id()?;
        obj.string_data = stream.read_ref_id()?;
        obj.canonical_names = stream.read_ref_id()?;
        obj.metadata_payloads = stream.read_ref_id()?;
        obj.metadata_mappings = stream.read_ref_id()?;
        obj.scripts = stream.read_ref_id()?;
        obj.constants = stream.read_ref_id()?;
        obj.constants_table = stream.read_ref_id()?;
        obj.libraries_cache = stream.read_ref_id()?;
        obj.classes_cache = stream.read_ref_id()?;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(UnlinkedCall, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.can_patch_to_monomorphic = stream.read_unsigned()? != 0;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(ICData, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.target_name = stream.read_ref_id()?;
        obj.args_descriptor = stream.read_ref_id()?;
        obj.entries = stream.read_ref_id()?;
        obj.state_bits = stream.read_unsigned()? as u32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(MegamorphicCache, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.target_name = stream.read_ref_id()?;
        obj.args_descriptor = stream.read_ref_id()?;
        obj.buckets = stream.read_ref_id()?;
        obj.mask = stream.read_ref_id()? as i32;
        obj.filled_entry_count = stream.read()? as i32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(SubtypeTestCache, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.cache = stream.read_ref_id()?;
        obj.num_inputs = stream.read_unsigned()? as u32;
        obj.num_occupied = stream.read_unsigned()? as u32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(LoadingUnit, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.parent = stream.read_ref_id()?;
        obj.base_objects = stream.read_ref_id()?;
        obj.packed_fields = stream.read()? as i64;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(LanguageError, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.previous_error = stream.read_ref_id()?;
        obj.script = stream.read_ref_id()?;
        obj.message = stream.read_ref_id()?;
        obj.formatted_message = stream.read_ref_id()?;
        obj.token_pos = stream.read()? as i32;
        obj.report_after_token = stream.read()? != 0;
        obj.kind = stream.read()? as i8;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(UnhandledException, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.exception = stream.read_ref_id()?;
        obj.stacktrace = stream.read_ref_id()?;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(LibraryPrefix, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.name = stream.read_ref_id()?;
        obj.imports = stream.read_ref_id()?;
        obj.importer = stream.read_ref_id()?;
        obj.num_imports = stream.read_unsigned()? as u16;
        obj.is_deferred_load = stream.read_unsigned()? != 0;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Type, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.type_test_stub = stream.read_ref_id()?;
        obj.hash = stream.read_ref_id()?;
        obj.arguments = stream.read_ref_id()?;
        obj.flags = stream.read_unsigned()? as u8;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(FunctionType, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.type_test_stub = stream.read_ref_id()?;
        obj.hash = stream.read_ref_id()?;
        obj.type_parameters = stream.read_ref_id()?;
        obj.result_type = stream.read_ref_id()?;
        obj.parameter_types = stream.read_ref_id()?;
        obj.named_parameter_names = stream.read_ref_id()?;
        obj.flags = stream.read()? as u8;
        obj.packed_parameter_counts = stream.read_unsigned()? as u32;
        obj.packed_type_parameter_counts = stream.read_unsigned()? as u16;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(RecordType, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.type_test_stub = stream.read_ref_id()?;
        obj.hash = stream.read_ref_id()?;
        obj.shape = stream.read_ref_id()? as i32;
        obj.field_types = stream.read_ref_id()?;
        obj.flags = stream.read()? as u8;
        // obj.shape = stream.read_ref_id()?; as i32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(TypeParameter, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.type_test_stub = stream.read_ref_id()?;
        obj.hash = stream.read_ref_id()?;
        obj.owner = stream.read_ref_id()?;
        obj.base = stream.read()? as u16;
        obj.index = stream.read()? as u16;
        obj.flags = stream.read()? as u8;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Closure, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.instantiator_type_arguments = stream.read_ref_id()?;
        obj.function_type_arguments = stream.read_ref_id()?;
        obj.delayed_type_arguments = stream.read_ref_id()?;
        obj.function = stream.read_ref_id()?;
        obj.context = stream.read_ref_id()?;
        obj.hash = stream.read_ref_id()?;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Double, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.value = f64::from_bits(stream.read_raw_u64()?);
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Int32x4, |_self, stream| {
    for _ in 0.._self.obj_count as usize {}
});
DECLARE_FIXED_LENGTH_CLUSTER!(GrowableObjectArray, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.type_arguments = stream.read_ref_id()?;
        obj.data = stream.read_ref_id()?;
        obj.length = stream.read_ref_id()? as i32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(TypedDataView, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.typed_data = stream.read_ref_id()?;
        obj.offset_in_bytes = stream.read_ref_id()? as i32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(ExternalTypedData, |_self, stream| {
    for _ in 0.._self.obj_count as usize {}
});
DECLARE_FIXED_LENGTH_CLUSTER!(StackTrace, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.async_link = stream.read_ref_id()?;
        obj.code_array = stream.read_ref_id()?;
        obj.pc_offset_array = stream.read_ref_id()?;
        // obj.expand_inlined = stream.read_unsigned()? != 0; [[NOT PRESENT IN FullAOT]]
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(RegExp, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.capture_name_map = stream.read_ref_id()?;
        obj.pattern = stream.read_ref_id()?;
        obj.one_byte = stream.read_ref_id()?;
        obj.two_byte = stream.read_ref_id()?;
        obj.one_byte_sticky = stream.read_ref_id()?;
        obj.two_byte_sticky = stream.read_ref_id()?;
        obj.num_one_byte_registers = stream.read()? as i32;
        obj.num_two_byte_registers = stream.read()? as i32;
        obj.flags = stream.read_unsigned()? as u32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(WeakProperty, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.key = stream.read_ref_id()?;
        obj.value = stream.read_ref_id()?;
        // obj.next_seen_by_gc = stream.read_ref_id()?; [[NOT PRESENT IN FullAOT]]
    }
});

DECLARE_VARIABLE_LENGTH_CLUSTER!(Map);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Set);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Instance);
DECLARE_VARIABLE_LENGTH_CLUSTER!(TypedData);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Class);
DECLARE_VARIABLE_LENGTH_CLUSTER!(TypeArguments);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Code);
DECLARE_VARIABLE_LENGTH_CLUSTER!(ObjectPool);
DECLARE_VARIABLE_LENGTH_CLUSTER!(ExceptionHandlers);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Context);
DECLARE_VARIABLE_LENGTH_CLUSTER!(ContextScope);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Mint);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Float32x4);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Float64x2);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Record);
DECLARE_VARIABLE_LENGTH_CLUSTER!(Array);
DECLARE_VARIABLE_LENGTH_CLUSTER!(WeakArray);
DECLARE_VARIABLE_LENGTH_CLUSTER!(ImmutableArray);
DECLARE_VARIABLE_LENGTH_CLUSTER!(ConstMap);
DECLARE_VARIABLE_LENGTH_CLUSTER!(ConstSet);
DECLARE_VARIABLE_LENGTH_CLUSTER!(CodeSourceMap);
DECLARE_VARIABLE_LENGTH_CLUSTER!(CompressedStackMaps);

impl Cluster for CompressedStackMapsCluster {
    fn is_fixed_len(&self) -> bool {
        false
    }

    fn read_alloc(&mut self, last_ref_id: &mut u64, stream: &mut Stream) -> anyhow::Result<usize> {
        self.start_of_alloc = stream.get_current_pos();
        self.first_ref_id = *last_ref_id as u32;

        self.obj_count = stream.read_unsigned()?;
        for _obj_idx in 0..self.obj_count {
            let mut obj = Box::<CompressedStackMaps>::default();
            obj.length = stream.read_unsigned()? as u32;

            self.objs.push(obj);
        }

        *last_ref_id += self.obj_count;
        self.end_of_alloc = stream.get_current_pos();

        Ok(self.end_of_alloc - self.start_of_alloc)
    }

    fn read_fill(&mut self, stream: &mut Stream) -> anyhow::Result<usize> {
        self.start_of_fill = stream.get_current_pos();
        for obj_idx in 0..self.obj_count {
            let obj = &mut self.objs[obj_idx as usize];
            obj.flags_and_size = stream.read_unsigned()? as u32;

            obj.data = stream.read_bytes(obj.length as usize)?;
        }

        self.end_of_fill = stream.get_current_pos();

        Ok(self.end_of_fill - self.start_of_fill)
    }
}

DECLARE_VARIABLE_LENGTH_CLUSTER!(PcDescriptors);

impl Cluster for PcDescriptorsCluster {
    fn is_fixed_len(&self) -> bool {
        false
    }

    fn read_alloc(&mut self, last_ref_id: &mut u64, stream: &mut Stream) -> anyhow::Result<usize> {
        self.start_of_alloc = stream.get_current_pos();
        self.first_ref_id = *last_ref_id as u32;

        self.obj_count = stream.read_unsigned()?;

        for _obj_idx in 0..self.obj_count {
            let mut obj = Box::<PcDescriptors>::default();
            let length = stream.read_unsigned()?;

            obj.length = length as u32;
            self.objs.push(obj);
        }

        *last_ref_id += self.obj_count;

        self.end_of_alloc = stream.get_current_pos();
        Ok(self.end_of_alloc - self.start_of_alloc)
    }

    fn read_fill(&mut self, stream: &mut Stream) -> anyhow::Result<usize> {
        self.start_of_fill = stream.get_current_pos();

        for obj_idx in 0..self.obj_count {
            let length = stream.read_unsigned()?; // its saved twice
            let variable_size_data = stream.read_bytes(length as usize)?;

            let obj = &mut self.objs[obj_idx as usize];
            obj.data = variable_size_data;
        }

        Ok(self.end_of_fill - self.start_of_fill)
    }
}

//DECLARE_VARIABLE_LENGTH_CLUSTER!(OneByteString); These only exist when NO COMPRESSED_POINTERS
//DECLARE_VARIABLE_LENGTH_CLUSTER!(TwoByteString);
DECLARE_VARIABLE_LENGTH_CLUSTER!(_String);

impl Cluster for _StringCluster {
    fn is_fixed_len(&self) -> bool {
        false
    }

    fn read_alloc(&mut self, last_ref_id: &mut u64, stream: &mut Stream) -> anyhow::Result<usize> {
        self.start_of_alloc = stream.get_current_pos();
        self.first_ref_id = *last_ref_id as u32;

        self.obj_count = stream.read_unsigned()?;

        for _obj_idx in 0..self.obj_count {
            let mut obj = Box::<_String>::default();
            let encoded = stream.read_unsigned()?;
            obj.string_type = if encoded & 1 == 1 {
                StrType::TwoByte
            } else {
                StrType::OneByte
            };

            obj.length = (encoded >> 1) as i32;
            self.objs.push(obj);
        }

        *last_ref_id += self.obj_count;
        self.end_of_alloc = stream.get_current_pos();

        Ok(self.end_of_alloc - self.start_of_alloc)
    }

    fn read_fill(&mut self, stream: &mut Stream) -> anyhow::Result<usize> {
        self.start_of_fill = stream.get_current_pos();

        for obj_idx in 0..self.obj_count {
            let _encoded = stream.read_unsigned()?; // why is this here twice? Huh?
            let obj = self.objs.get_mut(obj_idx as usize);

            let obj =
                obj.unwrap_or_else(|| panic!("Couldn't unwrap... No string at index {obj_idx}"));

            match obj.string_type {
                StrType::OneByte => {
                    // OneByteString payload is Latin-1, not UTF-8. Bytes 0x80..=0xFF
                    // are valid and map to U+0080..U+00FF; from_utf8 rejects them.
                    let mut decoded = String::with_capacity(obj.length as usize);
                    for _ in 0..obj.length {
                        decoded.push(stream.read_byte()? as char);
                    }
                    obj.internal_str = decoded;
                }
                StrType::TwoByte => {
                    // a TwoByteString has exactly `length` 16-bit code units
                    let mut code_units = Vec::with_capacity(obj.length as usize);

                    for _ in 0..obj.length {
                        // read 2 bytes (little-endian)
                        let b1 = stream.read_byte()? as u16;
                        let b2 = stream.read_byte()? as u16;
                        let code_unit = b1 | (b2 << 8);

                        code_units.push(code_unit);
                    }

                    // Dart strings may hold unpaired surrogates; never panic on them.
                    obj.internal_str = String::from_utf16_lossy(&code_units);
                }
            }
        }

        self.end_of_fill = stream.get_current_pos();
        Ok(self.end_of_fill - self.start_of_fill)
    }
}
