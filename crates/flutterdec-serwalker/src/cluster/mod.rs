use crate::constants::{ClassId, ClassId::*};
use crate::raw_object::*;
use crate::stream::Stream;
use crate::DECLARE_FIXED_LENGTH_CLUSTER;
use crate::DECLARE_VARIABLE_LENGTH_CLUSTER;
use crate::FFI_TYPES_LIST;

pub trait Cluster {
    fn set_metadata(&mut self, tags: u32, cid: ClassId, is_immutable: bool, is_canonical: bool);
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

DECLARE_FIXED_LENGTH_CLUSTER!(TypeParameters, TypeParametersCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.names = stream.read_ref_id()?;
        obj.flags = stream.read_ref_id()?;
        obj.bounds = stream.read_ref_id()?;
        obj.defaults = stream.read_ref_id()?;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(PatchClass, PatchClassCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.wrapped_class = stream.read_ref_id()?;
        obj.script = stream.read_ref_id()?;
        obj.kernel_program_info = stream.read_ref_id()?;
        // obj.kernel_library_index = stream.read_unsigned()? as u32; [[NOT PRESENT IN FullAOT]]
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Function, FunctionCluster, |_self, stream| {
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
        // obj.token_pos = stream.read()? as i32; [[NOT PRESENT IN release builds (called prouct in Flutter)]]
        // obj.kernel_offset = stream.read_unsigned()? as u32; [[NOT PRESENT IN FullAOT]]
        obj.kind_tag = stream.read()? as u32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(ClosureData, ClosureDataCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.context_scope = stream.read_ref_id()?;
        obj.parent_function = stream.read_ref_id()?;
        obj.closure = stream.read_ref_id()?;
        obj.packed_fields = stream.read_unsigned()? as u32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(
    FfiTrampolineData,
    FfiTrampolineDataCluster,
    |_self, stream| {
        for obj_idx in 0.._self.obj_count as usize {
            let obj = &mut *_self.objs[obj_idx];
            obj.signature_type = stream.read_ref_id()?;
            obj.c_signature = stream.read_ref_id()?;
            obj.callback_target = stream.read_ref_id()?;
            obj.callback_exceptional_return = stream.read_ref_id()?;
            obj.ffi_function_kind = stream.read_byte()? as u8;
            obj.callback_id = stream.read()? as i32;
        }
    }
);
DECLARE_FIXED_LENGTH_CLUSTER!(Field, FieldCluster, |_self, stream| {
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
DECLARE_FIXED_LENGTH_CLUSTER!(Script, ScriptCluster, |_self, stream| {
    for _ in 0.._self.obj_count as usize {}
});
DECLARE_FIXED_LENGTH_CLUSTER!(Library, LibraryCluster, |_self, stream| {
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
        obj.load_state = stream.read_byte()? as i8;
        obj.flags = stream.read_byte()? as u8;
        // obj.kernel_library_index = stream.read_unsigned()? as u32; [[NOT PRESENT IN FullAOT]]
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Namespace, NamespaceCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.target = stream.read_ref_id()?;
        obj.show_names = stream.read_ref_id()?;
        obj.hide_names = stream.read_ref_id()?;
        obj.owner = stream.read_ref_id()?;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(
    KernelProgramInfo,
    KernelProgramInfoCluster,
    |_self, stream| {
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
    }
);
DECLARE_FIXED_LENGTH_CLUSTER!(UnlinkedCall, UnlinkedCallCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.can_patch_to_monomorphic = stream.read_byte()? != 0;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(ICData, ICDataCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.target_name = stream.read_ref_id()?;
        obj.args_descriptor = stream.read_ref_id()?;
        obj.entries = stream.read_ref_id()?;
        obj.state_bits = stream.read_unsigned()? as u32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(
    MegamorphicCache,
    MegamorphicCacheCluster,
    |_self, stream| {
        for obj_idx in 0.._self.obj_count as usize {
            let obj = &mut *_self.objs[obj_idx];
            obj.target_name = stream.read_ref_id()?;
            obj.args_descriptor = stream.read_ref_id()?;
            obj.buckets = stream.read_ref_id()?;
            obj.mask = stream.read_ref_id()? as i32;
            obj.filled_entry_count = stream.read()? as i32;
        }
    }
);
DECLARE_FIXED_LENGTH_CLUSTER!(
    SubtypeTestCache,
    SubtypeTestCacheCluster,
    |_self, stream| {
        for obj_idx in 0.._self.obj_count as usize {
            let obj = &mut *_self.objs[obj_idx];
            obj.cache = stream.read_ref_id()?;
            obj.num_inputs = stream.read_unsigned()? as u32;
            obj.num_occupied = stream.read_unsigned()? as u32;
        }
    }
);
DECLARE_FIXED_LENGTH_CLUSTER!(LoadingUnit, LoadingUnitCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.parent = stream.read_ref_id()?;
        obj.base_objects = stream.read_ref_id()?;
        obj.packed_fields = stream.read()? as i64;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(LanguageError, LanguageErrorCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.previous_error = stream.read_ref_id()?;
        obj.script = stream.read_ref_id()?;
        obj.message = stream.read_ref_id()?;
        obj.formatted_message = stream.read_ref_id()?;
        obj.token_pos = stream.read()? as i32;
        obj.report_after_token = stream.read_byte()? != 0;
        obj.kind = stream.read_byte()? as i8;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(
    UnhandledException,
    UnhandledExceptionCluster,
    |_self, stream| {
        for obj_idx in 0.._self.obj_count as usize {
            let obj = &mut *_self.objs[obj_idx];
            obj.exception = stream.read_ref_id()?;
            obj.stacktrace = stream.read_ref_id()?;
        }
    }
);
DECLARE_FIXED_LENGTH_CLUSTER!(LibraryPrefix, LibraryPrefixCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.name = stream.read_ref_id()?;
        obj.imports = stream.read_ref_id()?;
        obj.importer = stream.read_ref_id()?;
        obj.num_imports = stream.read_unsigned()? as u16;
        obj.is_deferred_load = stream.read_byte()? != 0;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Type, TypeCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.type_test_stub = stream.read_ref_id()?;
        obj.hash = stream.read_ref_id()?;
        obj.arguments = stream.read_ref_id()?;
        obj.flags = stream.read_unsigned()? as u8;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(FunctionType, FunctionTypeCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.type_test_stub = stream.read_ref_id()?;
        obj.hash = stream.read_ref_id()?;
        obj.type_parameters = stream.read_ref_id()?;
        obj.result_type = stream.read_ref_id()?;
        obj.parameter_types = stream.read_ref_id()?;
        obj.named_parameter_names = stream.read_ref_id()?;
        obj.flags = stream.read_byte()? as u8;
        obj.packed_parameter_counts = stream.read_unsigned()? as u32;
        obj.packed_type_parameter_counts = stream.read_unsigned()? as u16;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(RecordType, RecordTypeCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.type_test_stub = stream.read_ref_id()?;
        obj.hash = stream.read_ref_id()?;
        obj.shape = stream.read_ref_id()? as i32;
        obj.field_types = stream.read_ref_id()?;
        obj.flags = stream.read_byte()? as u8;
        // obj.shape = stream.read_ref_id()?; as i32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(TypeParameter, TypeParameterCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.type_test_stub = stream.read_ref_id()?;
        obj.hash = stream.read_ref_id()?;
        obj.owner = stream.read_ref_id()?;
        obj.base = stream.read()? as u16;
        obj.index = stream.read()? as u16;
        obj.flags = stream.read_byte()? as u8;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Closure, ClosureCluster, |_self, stream| {
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
DECLARE_FIXED_LENGTH_CLUSTER!(Double, DoubleCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.value = f64::from_bits(stream.read_raw_u64()?);
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Int32x4, Int32x4Cluster, |_self, stream| {
    for _ in 0.._self.obj_count as usize {}
});
DECLARE_FIXED_LENGTH_CLUSTER!(
    GrowableObjectArray,
    GrowableObjectArrayCluster,
    |_self, stream| {
        for obj_idx in 0.._self.obj_count as usize {
            let obj = &mut *_self.objs[obj_idx];
            obj.type_arguments = stream.read_ref_id()?;
            obj.data = stream.read_ref_id()?;
            obj.length = stream.read_ref_id()? as i32;
        }
    }
);
DECLARE_FIXED_LENGTH_CLUSTER!(TypedDataView, TypedDataViewCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.typed_data = stream.read_ref_id()?;
        obj.offset_in_bytes = stream.read_ref_id()? as i32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(
    ExternalTypedData,
    ExternalTypedDataCluster,
    |_self, stream| { for _ in 0.._self.obj_count as usize {} }
);
DECLARE_FIXED_LENGTH_CLUSTER!(StackTrace, StackTraceCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.async_link = stream.read_ref_id()?;
        obj.code_array = stream.read_ref_id()?;
        obj.pc_offset_array = stream.read_ref_id()?;
        // obj.expand_inlined = stream.read_unsigned()? != 0; [[NOT PRESENT IN FullAOT]]
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(RegExp, RegExpCluster, |_self, stream| {
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
        // Deserializer::Read<int8_t>() dispatches to ReadStream::Raw<1, T>.
        obj.type_flags = stream.read_byte()? as i8;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(WeakProperty, WeakPropertyCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.key = stream.read_ref_id()?;
        obj.value = stream.read_ref_id()?;
        // obj.next_seen_by_gc = stream.read_ref_id()?; [[NOT PRESENT IN FullAOT]]
    }
});

DECLARE_VARIABLE_LENGTH_CLUSTER!(Code, CodeCluster);
DECLARE_VARIABLE_LENGTH_CLUSTER!(ObjectPool, ObjectPoolCluster);

macro_rules! IMPLEMENT_VARIABLE_LENGTH_CLUSTER {
    (
        $cluster_name:ident,
        |$alloc_self:ident, $alloc_stream:ident| $alloc_impl:block,
        |$fill_self:ident, $fill_stream:ident| $fill_impl:block
    ) => {
        impl Cluster for $cluster_name {
            fn set_metadata(
                &mut self,
                tags: u32,
                cid: ClassId,
                is_immutable: bool,
                is_canonical: bool,
            ) {
                self.tags = tags;
                self.cid = cid;
                self.is_immutable = is_immutable;
                self.is_canonical = is_canonical;
            }

            fn is_fixed_len(&self) -> bool {
                false
            }

            fn read_alloc(
                &mut self,
                last_ref_id: &mut u64,
                stream: &mut Stream,
            ) -> anyhow::Result<usize> {
                self.start_of_alloc = stream.get_current_pos();
                self.first_ref_id = *last_ref_id as u32;

                let $alloc_self = &mut *self;
                let $alloc_stream = &mut *stream;
                $alloc_impl

                *last_ref_id += $alloc_self.obj_count;
                $alloc_self.end_of_alloc = $alloc_stream.get_current_pos();
                Ok($alloc_self.end_of_alloc - $alloc_self.start_of_alloc)
            }

            fn read_fill(&mut self, stream: &mut Stream) -> anyhow::Result<usize> {
                self.start_of_fill = stream.get_current_pos();

                let $fill_self = &mut *self;
                let $fill_stream = &mut *stream;
                $fill_impl

                $fill_self.end_of_fill = $fill_stream.get_current_pos();
                Ok($fill_self.end_of_fill - $fill_self.start_of_fill)
            }
        }
    };
}

fn typed_data_element_size(cid: ClassId) -> anyhow::Result<usize> {
    let size = match cid {
        TypedDataInt8ArrayCid | TypedDataUint8ArrayCid | TypedDataUint8ClampedArrayCid => 1,
        TypedDataInt16ArrayCid | TypedDataUint16ArrayCid => 2,
        TypedDataInt32ArrayCid | TypedDataUint32ArrayCid | TypedDataFloat32ArrayCid => 4,
        TypedDataInt64ArrayCid | TypedDataUint64ArrayCid | TypedDataFloat64ArrayCid => 8,
        TypedDataFloat32x4ArrayCid | TypedDataInt32x4ArrayCid | TypedDataFloat64x2ArrayCid => 16,
        _ => anyhow::bail!("class {:?} is not an internal TypedData class", cid),
    };
    Ok(size)
}

DECLARE_VARIABLE_LENGTH_CLUSTER!(Map, MapCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    MapCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            cluster.objs.push(Box::<Map>::default());
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            obj.type_arguments = stream.read_ref_id()?;
            obj.hash_mask = stream.read_ref_id()?;
            obj.data = stream.read_ref_id()?;
            obj.used_data = stream.read_ref_id()?;
            obj.deleted_keys = stream.read_ref_id()?;
            // UntaggedLinkedHashBase::to_snapshot excludes the rebuilt index_.
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Set, SetCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    SetCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            cluster.objs.push(Box::<Set>::default());
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            obj.type_arguments = stream.read_ref_id()?;
            obj.hash_mask = stream.read_ref_id()?;
            obj.data = stream.read_ref_id()?;
            obj.used_data = stream.read_ref_id()?;
            obj.deleted_keys = stream.read_ref_id()?;
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Instance, InstanceCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    InstanceCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        let next_field_offset_in_words = stream.read()? as i32;
        let instance_size_in_words = stream.read()? as i32;
        for _ in 0..cluster.obj_count {
            cluster.objs.push(Box::new(Instance {
                next_field_offset_in_words,
                instance_size_in_words,
                ..Instance::default()
            }));
        }
    },
    |cluster, stream| {
        const FIRST_INSTANCE_FIELD_WORD: i32 = 2;

        let bitmap = stream.read_unsigned()?;
        for obj in &mut cluster.objs {
            obj.unboxed_fields_bitmap = bitmap;
            if obj.next_field_offset_in_words < FIRST_INSTANCE_FIELD_WORD {
                anyhow::bail!(
                    "invalid instance next-field offset {}",
                    obj.next_field_offset_in_words
                );
            }

            for word_index in FIRST_INSTANCE_FIELD_WORD..obj.next_field_offset_in_words {
                let mask = 1_u64.checked_shl(word_index as u32).unwrap_or(0);
                if bitmap & mask != 0 {
                    // ReadWordWith32BitReads reads two encoded 32-bit chunks
                    // for a 64-bit target word.
                    let low = stream.read()? as u32 as u64;
                    let high = stream.read()? as u32 as u64;
                    obj.fields.push(InstanceField::Unboxed(low | (high << 32)));
                } else {
                    obj.fields
                        .push(InstanceField::Reference(stream.read_ref_id()?));
                }
            }
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(TypedData, TypedDataCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    TypedDataCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            let length = stream.read_unsigned()? as usize;
            cluster.objs.push(Box::new(TypedData {
                length,
                ..TypedData::default()
            }));
        }
    },
    |cluster, stream| {
        let element_size = typed_data_element_size(cluster.cid)?;
        for obj in &mut cluster.objs {
            let length = stream.read_unsigned()? as usize;
            let byte_length = length
                .checked_mul(element_size)
                .ok_or_else(|| anyhow::anyhow!("TypedData byte length overflow"))?;
            obj.length = length;
            obj.data = stream.read_bytes(byte_length)?;
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Class, ClassCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    ClassCluster,
    |cluster, stream| {
        let predefined_count = stream.read_unsigned()?;
        for _ in 0..predefined_count {
            cluster.objs.push(Box::new(Class {
                id: stream.read()? as i32,
                is_predefined: true,
                ..Class::default()
            }));
        }

        let regular_count = stream.read_unsigned()?;
        for _ in 0..regular_count {
            cluster.objs.push(Box::<Class>::default());
        }
        cluster.obj_count = predefined_count + regular_count;
    },
    |cluster, stream| {
        const TOP_LEVEL_CID_OFFSET: i32 = 1 << 20;

        for obj in &mut cluster.objs {
            obj.name = stream.read_ref_id()?;
            obj.functions = stream.read_ref_id()?;
            obj.functions_hash_table = stream.read_ref_id()?;
            obj.fields = stream.read_ref_id()?;
            obj.offset_in_words_to_field = stream.read_ref_id()?;
            obj.interfaces = stream.read_ref_id()?;
            obj.script = stream.read_ref_id()?;
            obj.library = stream.read_ref_id()?;
            obj.type_parameters = stream.read_ref_id()?;
            obj.super_type = stream.read_ref_id()?;
            obj.constants = stream.read_ref_id()?;
            obj.declaration_type = stream.read_ref_id()?;
            obj.invocation_dispatcher_cache = stream.read_ref_id()?;

            obj.id = stream.read()? as i32;
            obj.target_instance_size_in_words = stream.read()? as i32;
            obj.target_next_field_offset_in_words = stream.read()? as i32;
            obj.target_type_arguments_field_offset_in_words = stream.read()? as i32;
            obj.num_type_arguments = stream.read()? as i16;
            obj.num_native_fields = stream.read()? as u16;
            obj.state_bits = stream.read()? as u32;
            if obj.id < TOP_LEVEL_CID_OFFSET {
                obj.unboxed_fields_bitmap = Some(stream.read_unsigned()?);
            }
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(TypeArguments, TypeArgumentsCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    TypeArgumentsCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            let length = stream.read_unsigned()? as i32;
            cluster.objs.push(Box::new(TypeArguments {
                length,
                ..TypeArguments::default()
            }));
        }

        // a canonical cluster in the root loading unit carries the canonical
        // hash-set layout after its ordinary allocation records

        // so this is just for the canonical TypeArguments cluster (if any)
        if cluster.is_canonical {
            let _table_length = stream.read_unsigned()?;
            let first_element = stream.read_unsigned()?;
            if first_element > cluster.obj_count {
                anyhow::bail!(
                    "canonical TypeArguments first element {first_element} exceeds count {}",
                    cluster.obj_count
                );
            }
            for _ in first_element..cluster.obj_count {
                let _gap = stream.read_unsigned()?;
            }
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            let length = stream.read_unsigned()? as i32;
            obj.length = length;
            obj.hash = stream.read()? as i32;
            obj.nullability = stream.read_unsigned()? as i32;
            obj.instantiations = stream.read_ref_id()?;
            for _ in 0..length {
                obj.types.push(stream.read_ref_id()?);
            }
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(ExceptionHandlers, ExceptionHandlersCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    ExceptionHandlersCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            let num_entries = stream.read_unsigned()? as usize;
            cluster.objs.push(Box::new(ExceptionHandlers {
                num_entries,
                ..ExceptionHandlers::default()
            }));
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            obj.packed_fields = stream.read_unsigned()? as u32;
            obj.handled_types_data = stream.read_ref_id()?;
            for _ in 0..obj.num_entries {
                obj.entries.push(ExceptionHandlerInfo {
                    handler_pc_offset: stream.read()? as u32,
                    outer_try_index: stream.read()? as i16,
                    needs_stacktrace: stream.read_byte()? as i8,
                    has_catch_all: stream.read_byte()? as i8,
                    is_generated: stream.read_byte()? as i8,
                });
            }
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Context, ContextCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    ContextCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            let num_variables = stream.read_unsigned()? as i32;
            cluster.objs.push(Box::new(Context {
                num_variables,
                ..Context::default()
            }));
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            let num_variables = stream.read_unsigned()? as i32;
            obj.num_variables = num_variables;
            obj.parent = stream.read_ref_id()?;
            for _ in 0..num_variables {
                obj.variables.push(stream.read_ref_id()?);
            }
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(ContextScope, ContextScopeCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    ContextScopeCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            let num_variables = stream.read_unsigned()? as i32;
            cluster.objs.push(Box::new(ContextScope {
                num_variables,
                ..ContextScope::default()
            }));
        }
    },
    |cluster, stream| {
        const VARIABLE_DESC_REF_COUNT: i32 = 10;

        for obj in &mut cluster.objs {
            let num_variables = stream.read_unsigned()? as i32;
            obj.num_variables = num_variables;
            obj.is_implicit = stream.read_byte()? != 0;
            for _ in 0..num_variables * VARIABLE_DESC_REF_COUNT {
                obj.variables.push(stream.read_ref_id()?);
            }
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Mint, MintCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    MintCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            cluster.objs.push(Box::new(Mint {
                value: stream.read()? as i64,
            }));
        }
    },
    |_cluster, _stream| {}
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Float32x4, Float32x4Cluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    Float32x4Cluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            cluster.objs.push(Box::<Float32x4>::default());
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            obj.value = stream.read_bytes(16)?;
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Float64x2, Float64x2Cluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    Float64x2Cluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            cluster.objs.push(Box::<Float64x2>::default());
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            obj.value = stream.read_bytes(16)?;
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Record, RecordCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    RecordCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            let num_fields = stream.read_unsigned()? as usize;
            cluster.objs.push(Box::new(Record {
                num_fields,
                ..Record::default()
            }));
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            obj.shape = stream.read_unsigned()? as i32;
            for _ in 0..obj.num_fields {
                obj.fields.push(stream.read_ref_id()?);
            }
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Array, ArrayCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    ArrayCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            let length = stream.read_unsigned()? as i32;
            cluster.objs.push(Box::new(Array {
                length,
                ..Array::default()
            }));
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            let length = stream.read_unsigned()? as i32;
            obj.length = length;
            obj.type_arguments = stream.read_ref_id()?;
            for _ in 0..length {
                obj.elements.push(stream.read_ref_id()?);
            }
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(WeakArray, WeakArrayCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    WeakArrayCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            let length = stream.read_unsigned()? as i32;
            cluster.objs.push(Box::new(WeakArray {
                length,
                ..WeakArray::default()
            }));
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            let length = stream.read_unsigned()? as i32;
            obj.length = length;
            for _ in 0..length {
                obj.elements.push(stream.read_ref_id()?);
            }
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(ImmutableArray, ImmutableArrayCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    ImmutableArrayCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            let length = stream.read_unsigned()? as i32;
            cluster.objs.push(Box::new(ImmutableArray {
                length,
                ..ImmutableArray::default()
            }));
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            let length = stream.read_unsigned()? as i32;
            obj.length = length;
            obj.type_arguments = stream.read_ref_id()?;
            for _ in 0..length {
                obj.elements.push(stream.read_ref_id()?);
            }
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(ConstMap, ConstMapCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    ConstMapCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            cluster.objs.push(Box::<ConstMap>::default());
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            obj.type_arguments = stream.read_ref_id()?;
            obj.hash_mask = stream.read_ref_id()?;
            obj.data = stream.read_ref_id()?;
            obj.used_data = stream.read_ref_id()?;
            obj.deleted_keys = stream.read_ref_id()?;
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(ConstSet, ConstSetCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    ConstSetCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            cluster.objs.push(Box::<ConstSet>::default());
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            obj.type_arguments = stream.read_ref_id()?;
            obj.hash_mask = stream.read_ref_id()?;
            obj.data = stream.read_ref_id()?;
            obj.used_data = stream.read_ref_id()?;
            obj.deleted_keys = stream.read_ref_id()?;
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(CodeSourceMap, CodeSourceMapCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    CodeSourceMapCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            let length = stream.read_unsigned()? as u32;
            cluster.objs.push(Box::new(CodeSourceMap {
                length,
                ..CodeSourceMap::default()
            }));
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            let length = stream.read_unsigned()? as u32;
            obj.length = length;
            obj.data = stream.read_bytes(length as usize)?;
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(CompressedStackMaps, CompressedStackMapsCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    CompressedStackMapsCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            let length = stream.read_unsigned()? as u32;
            cluster.objs.push(Box::new(CompressedStackMaps {
                length,
                ..CompressedStackMaps::default()
            }));
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            obj.flags_and_size = stream.read_unsigned()? as u32;
            obj.data = stream.read_bytes(obj.length as usize)?;
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(PcDescriptors, PcDescriptorsCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    PcDescriptorsCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            let length = stream.read_unsigned()? as u32;
            cluster.objs.push(Box::new(PcDescriptors {
                length,
                ..PcDescriptors::default()
            }));
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            let length = stream.read_unsigned()?; // its saved twice
            obj.data = stream.read_bytes(length as usize)?;
        }
    }
);

//DECLARE_VARIABLE_LENGTH_CLUSTER!(OneByteString, OneByteStringCluster); These only exist when NO COMPRESSED_POINTERS
//DECLARE_VARIABLE_LENGTH_CLUSTER!(TwoByteString, TwoByteStringCluster);
DECLARE_VARIABLE_LENGTH_CLUSTER!(_String, _StringCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    _StringCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            let encoded = stream.read_unsigned()?;
            let string_type = if encoded & 1 == 1 {
                StrType::TwoByte
            } else {
                StrType::OneByte
            };
            cluster.objs.push(Box::new(_String {
                string_type,
                length: (encoded >> 1) as i32,
                .._String::default()
            }));
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            let _encoded = stream.read_unsigned()?; // why is this here twice? Huh?
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
    }
);
