mod base_objects_pseudocluster;

use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::{any, u64};

use crate::constants::{Cid, ClassId, ClassId::*};
use crate::instruction_table::{get_pc_offset_from_code_cluster_index, InstructionTable};
use crate::raw_object::*;
use crate::stream::Stream;
use crate::DECLARE_FIXED_LENGTH_CLUSTER;
use crate::DECLARE_VARIABLE_LENGTH_CLUSTER;

pub trait Cluster {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
    fn as_any(&self) -> &dyn std::any::Any;
    fn object_by_ref_id(&self, id: u32) -> Option<&dyn std::any::Any>;
    fn set_metadata(&mut self, tags: u32, cid: Cid, is_immutable: bool, is_canonical: bool);
    fn is_fixed_len(&self) -> bool;
    fn read_alloc(&mut self, last_ref_id: &mut u64, stream: &mut Stream) -> anyhow::Result<usize>;
    fn read_fill(&mut self, stream: &mut Stream) -> anyhow::Result<usize>;
}

pub fn read_smi(stream: &mut Stream) -> anyhow::Result<Smi> {
    let raw_smi = stream.read()?; // smis are always written as signed numbers

    Ok(raw_smi as Smi)
}

fn is_typed_data_cid(cid: Cid, remainder: Cid) -> bool {
    let first = TypedDataInt8ArrayCid as Cid;
    let last = UnmodifiableTypedDataFloat64x2ArrayViewCid as Cid;
    (first..=last).contains(&cid) && (cid - first) % 4 == remainder
}

pub fn decide_cluster(cid: Cid) -> Result<Box<dyn Cluster>, &'static str> {
    if cid == NativePointer as Cid {
        return Ok(Box::new(DeltaEncodedTypedDataCluster::default()));
    }
    if cid >= NumPredefinedCids as Cid || cid == InstanceCid as Cid {
        return Ok(Box::new(InstanceCluster::default()));
    }
    if is_typed_data_cid(cid, 1) || cid == ByteDataViewCid as Cid {
        return Ok(Box::new(TypedDataViewCluster::default()));
    }
    if is_typed_data_cid(cid, 2) {
        return Ok(Box::new(ExternalTypedDataCluster::default()));
    }
    if is_typed_data_cid(cid, 0) {
        return Ok(Box::new(TypedDataCluster::default()));
    }

    match ClassId::try_from(cid).map_err(|_| "CID is neither predefined nor an application CID")? {
        ClassCid => Ok(Box::new(ClassCluster::default())),
        TypeParametersCid => Ok(Box::new(TypeParametersCluster::default())),
        TypeArgumentsCid => Ok(Box::new(TypeArgumentsCluster::default())),
        PatchClassCid => Ok(Box::new(PatchClassCluster::default())),
        FunctionCid => Ok(Box::new(FunctionCluster::default())),
        ClosureDataCid => Ok(Box::new(ClosureDataCluster::default())),
        FfiTrampolineDataCid => Ok(Box::new(FfiTrampolineDataCluster::default())),
        FieldCid => Ok(Box::new(FieldCluster::default())),
        ScriptCid => Ok(Box::new(ScriptCluster::default())),
        LibraryCid => Ok(Box::new(LibraryCluster::default())),
        NamespaceCid => Ok(Box::new(NamespaceCluster::default())),
        CodeCid => Ok(Box::new(CodeCluster::default())),
        ObjectPoolCid => Ok(Box::new(ObjectPoolCluster::default())),
        PcDescriptorsCid => Ok(Box::new(PcDescriptorsCluster::default())),
        CodeSourceMapCid => Ok(Box::new(CodeSourceMapCluster::default())),
        CompressedStackMapsCid => Ok(Box::new(CompressedStackMapsCluster::default())),
        ExceptionHandlersCid => Ok(Box::new(ExceptionHandlersCluster::default())),
        ContextCid => Ok(Box::new(ContextCluster::default())),
        ContextScopeCid => Ok(Box::new(ContextScopeCluster::default())),
        UnlinkedCallCid => Ok(Box::new(UnlinkedCallCluster::default())),
        ICDataCid => Ok(Box::new(ICDataCluster::default())),
        MegamorphicCacheCid => Ok(Box::new(MegamorphicCacheCluster::default())),
        SubtypeTestCacheCid => Ok(Box::new(SubtypeTestCacheCluster::default())),
        LoadingUnitCid => Ok(Box::new(LoadingUnitCluster::default())),
        LanguageErrorCid => Ok(Box::new(LanguageErrorCluster::default())),
        UnhandledExceptionCid => Ok(Box::new(UnhandledExceptionCluster::default())),
        LibraryPrefixCid => Ok(Box::new(LibraryPrefixCluster::default())),
        TypeCid => Ok(Box::new(TypeCluster::default())),
        FunctionTypeCid => Ok(Box::new(FunctionTypeCluster::default())),
        RecordTypeCid => Ok(Box::new(RecordTypeCluster::default())),
        TypeParameterCid => Ok(Box::new(TypeParameterCluster::default())),
        ClosureCid => Ok(Box::new(ClosureCluster::default())),
        MintCid => Ok(Box::new(MintCluster::default())),
        DoubleCid => Ok(Box::new(DoubleCluster::default())),
        Int32x4Cid => Ok(Box::new(Int32x4Cluster::default())),
        Float32x4Cid => Ok(Box::new(Float32x4Cluster::default())),
        Float64x2Cid => Ok(Box::new(Float64x2Cluster::default())),
        GrowableObjectArrayCid => Ok(Box::new(GrowableObjectArrayCluster::default())),
        RecordCid => Ok(Box::new(RecordCluster::default())),
        StackTraceCid => Ok(Box::new(StackTraceCluster::default())),
        RegExpCid => Ok(Box::new(RegExpCluster::default())),
        WeakPropertyCid => Ok(Box::new(WeakPropertyCluster::default())),
        ConstMapCid => Ok(Box::new(ConstMapCluster::default())),
        ConstSetCid => Ok(Box::new(ConstSetCluster::default())),
        ArrayCid => Ok(Box::new(ArrayCluster::default())),
        ImmutableArrayCid => Ok(Box::new(ImmutableArrayCluster::default())),
        WeakArrayCid => Ok(Box::new(WeakArrayCluster::default())),
        _StringCid => Ok(Box::new(_StringCluster::default())),
        FfiInt8Cid | FfiInt16Cid | FfiInt32Cid | FfiInt64Cid | FfiUint8Cid | FfiUint16Cid
        | FfiUint32Cid | FfiUint64Cid | FfiFloatCid | FfiDoubleCid | FfiVoidCid | FfiHandleCid
        | FfiBoolCid => Ok(Box::new(InstanceCluster::default())),
        IllegalCid => Err("illegal CID cannot identify a serialized cluster"),
        MapCid | SetCid => Err("mutable Map and Set clusters are unreachable in snapshots"),
        _ => Err("no FullAOT compressed-pointer cluster exists for this predefined CID"),
    }
}

pub(crate) fn read_canonical_set_layout(obj_count: u64, stream: &mut Stream) -> anyhow::Result<()> {
    let _table_length = stream.read_unsigned()?;
    let first_element = stream.read_unsigned()?;
    anyhow::ensure!(
        first_element <= obj_count,
        "canonical-set first element {first_element} exceeds object count {obj_count}"
    );

    for _ in first_element..obj_count {
        let _gap = stream.read_unsigned()?;
    }

    Ok(())
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
        // obj.kernel_library_index = stream.read_unsigned()? as u32; [[NOT PRESENT IN FullAOT]]
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Function, FunctionCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.entry_point = u64::MAX;
        obj.unchecked_entry_point = u64::MAX;
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
            obj.callback_id = stream.read()? as i32;
            obj.ffi_function_kind = stream.read_byte()? as u8;
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
        // obj.guarded_list_length = stream.read_ref_id()?; [[NOT PRESENT IN FullAOT]]
        // obj.exact_type = stream.read_ref_id()?; [[NOT PRESENT IN FullAOT]]
        // obj.dependent_code = stream.read_ref_id()?; [[NOT PRESENT IN FullAOT]]
        // obj.token_pos = stream.read()? as i32; [[NOT PRESENT IN FullAOT]]
        // obj.end_token_pos = stream.read()? as i32; [[NOT PRESENT IN FullAOT]]
        // obj.guarded_cid = stream.read()? as u32; [[NOT PRESENT IN FullAOT]]
        // obj.is_nullable = stream.read()? as u32; [[NOT PRESENT IN FullAOT]]
        // obj.kernel_offset = stream.read()? as u32; [[NOT PRESENT IN FullAOT]]
        // obj.guarded_list_length_in_object_offset = stream.read_byte()? as i8; [[NOT PRESENT IN FullAOT]]
        // obj.static_type_exactness_state = stream.read_byte()? as i8; [[NOT PRESENT IN FullAOT]]
        // obj.target_offset = stream.read()? as i32; [[NOT PRESENT IN FullAOT]]
        obj.kind_bits = stream.read()? as u32;
        obj.host_offset_or_field_id = stream.read_ref_id()?;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Script, ScriptCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.url = stream.read_ref_id()?;
        obj.kernel_script_index = stream.read()? as i32;
    }
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
        obj.target_name = stream.read_ref_id()?;
        obj.args_descriptor = stream.read_ref_id()?;
        obj.can_patch_to_monomorphic = stream.read_byte()? != 0;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(ICData, ICDataCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.target_name = stream.read_ref_id()?;
        obj.args_descriptor = stream.read_ref_id()?;
        obj.entries = stream.read_ref_id()?;
        obj.state_bits = stream.read()? as u32;
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
            obj.num_inputs = stream.read()? as u32;
            obj.num_occupied = stream.read()? as u32;
        }
    }
);
DECLARE_FIXED_LENGTH_CLUSTER!(LoadingUnit, LoadingUnitCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.parent = stream.read_ref_id()?;
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
        obj.num_imports = stream.read()? as u16;
        obj.is_deferred_load = stream.read_byte()? != 0;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Type, TypeCluster, true, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.type_test_stub = stream.read_ref_id()?;
        obj.hash = stream.read_ref_id()?;
        obj.arguments = stream.read_ref_id()?;
        obj.flags = stream.read_unsigned()?.try_into()?;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(FunctionType, FunctionTypeCluster, true, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.type_test_stub = stream.read_ref_id()?;
        obj.hash = stream.read_ref_id()?;
        obj.type_parameters = stream.read_ref_id()?;
        obj.result_type = stream.read_ref_id()?;
        obj.parameter_types = stream.read_ref_id()?;
        obj.named_parameter_names = stream.read_ref_id()?;
        obj.flags = stream.read_byte()? as u8;
        obj.packed_parameter_counts = stream.read()? as u32;
        obj.packed_type_parameter_counts = stream.read()? as u16;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(RecordType, RecordTypeCluster, true, |_self, stream| {
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
DECLARE_FIXED_LENGTH_CLUSTER!(
    TypeParameter,
    TypeParameterCluster,
    true,
    |_self, stream| {
        for obj_idx in 0.._self.obj_count as usize {
            let obj = &mut *_self.objs[obj_idx];
            obj.type_test_stub = stream.read_ref_id()?;
            obj.hash = stream.read_ref_id()?;
            obj.owner = stream.read_ref_id()?;
            obj.base = stream.read()? as u16;
            obj.index = stream.read()? as u16;
            obj.flags = stream.read_byte()? as u8;
        }
    }
);
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
        obj.value = f64::from_bits(stream.read()?);
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(Int32x4, Int32x4Cluster, |_self, stream| {
    for obj in &mut _self.objs {
        obj.value = stream.read_bytes(16)?;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(
    GrowableObjectArray,
    GrowableObjectArrayCluster,
    |_self, stream| {
        for obj_idx in 0.._self.obj_count as usize {
            let obj = &mut *_self.objs[obj_idx];
            obj.type_arguments = stream.read_ref_id()?;
            obj.length = stream.read_ref_id()? as i32;
            obj.data = stream.read_ref_id()?;
        }
    }
);
DECLARE_FIXED_LENGTH_CLUSTER!(TypedDataView, TypedDataViewCluster, |_self, stream| {
    for obj_idx in 0.._self.obj_count as usize {
        let obj = &mut *_self.objs[obj_idx];
        obj.length = stream.read_ref_id()? as i32;
        obj.typed_data = stream.read_ref_id()?;
        obj.offset_in_bytes = stream.read_ref_id()? as i32;
    }
});
DECLARE_FIXED_LENGTH_CLUSTER!(
    ExternalTypedData,
    ExternalTypedDataCluster,
    |_self, stream| {
        let element_size = typed_data_element_size(_self.cid)?;
        for obj in &mut _self.objs {
            let length = usize::try_from(stream.read_unsigned()?)
                .map_err(|_| anyhow::anyhow!("ExternalTypedData length does not fit in usize"))?;
            let byte_length = length
                .checked_mul(element_size)
                .ok_or_else(|| anyhow::anyhow!("ExternalTypedData byte length overflow"))?;
            stream.align_stream(8)?;
            obj.length = length;
            obj.data = stream.read_bytes(byte_length)?;
        }
    }
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

macro_rules! IMPLEMENT_VARIABLE_LENGTH_CLUSTER {
    (
        $cluster_name:ident,
        |$alloc_self:ident, $alloc_stream:ident| $alloc_impl:block,
        |$fill_self:ident, $fill_stream:ident| $fill_impl:block
    ) => {
        impl Cluster for $cluster_name {
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn object_by_ref_id(&self, id: u32) -> Option<&dyn std::any::Any> {
                let index = id.checked_sub(self.first_ref_id)? as usize; // turn the global id into a local one
                self.objs
                    .get(index)
                    .map(|object| object.as_ref() as &dyn std::any::Any)
            }

            fn set_metadata(
                &mut self,
                tags: u32,
                cid: Cid,
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

fn typed_data_element_size(cid: Cid) -> anyhow::Result<usize> {
    let class_id = ClassId::try_from(cid)
        .map_err(|_| anyhow::anyhow!("application CID {cid} is not a TypedData class"))?;
    let size = match class_id {
        TypedDataInt8ArrayCid
        | TypedDataUint8ArrayCid
        | TypedDataUint8ClampedArrayCid
        | ExternalTypedDataInt8ArrayCid
        | ExternalTypedDataUint8ArrayCid
        | ExternalTypedDataUint8ClampedArrayCid => 1,
        TypedDataInt16ArrayCid
        | TypedDataUint16ArrayCid
        | ExternalTypedDataInt16ArrayCid
        | ExternalTypedDataUint16ArrayCid => 2,
        TypedDataInt32ArrayCid
        | TypedDataUint32ArrayCid
        | TypedDataFloat32ArrayCid
        | ExternalTypedDataInt32ArrayCid
        | ExternalTypedDataUint32ArrayCid
        | ExternalTypedDataFloat32ArrayCid => 4,
        TypedDataInt64ArrayCid
        | TypedDataUint64ArrayCid
        | TypedDataFloat64ArrayCid
        | ExternalTypedDataInt64ArrayCid
        | ExternalTypedDataUint64ArrayCid
        | ExternalTypedDataFloat64ArrayCid => 8,
        TypedDataFloat32x4ArrayCid
        | TypedDataInt32x4ArrayCid
        | TypedDataFloat64x2ArrayCid
        | ExternalTypedDataFloat32x4ArrayCid
        | ExternalTypedDataInt32x4ArrayCid
        | ExternalTypedDataFloat64x2ArrayCid => 16,
        _ => anyhow::bail!("class {:?} is not a supported TypedData class", class_id),
    };
    Ok(size)
}

#[derive(Default)]
pub struct CodeCluster {
    tags: u32,
    cid: Cid,
    is_immutable: bool,
    is_canonical: bool,
    obj_count: u64,
    non_deferred_obj_count: u64,
    deferred_obj_count: u64,

    start_of_fill: usize,
    start_of_alloc: usize,

    end_of_fill: usize,
    end_of_alloc: usize,

    pub(crate) first_ref_id: u32,

    objs: Vec<Box<Code>>,
}

IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    CodeCluster,
    |cluster, stream| {
        cluster.non_deferred_obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.non_deferred_obj_count {
            // for normal code objects
            let mut code = Box::<Code>::default();
            code.state_bits = stream.read()? as i32;
            cluster.objs.push(code);
        }

        cluster.deferred_obj_count = stream.read_unsigned()?; // number of deferred objs
        for _ in 0..cluster.deferred_obj_count {
            // for deferred code objects
            let mut code = Box::<Code>::default();
            code.state_bits = stream.read()? as i32;
            cluster.objs.push(code);
        }

        cluster.obj_count = cluster.non_deferred_obj_count + cluster.deferred_obj_count;
    },
    |cluster, stream| {
        // this is the ARM-64-specific offsets, in the future we must add the other architecture's offsets if we want to support them
        // const MONOMORPHIC_ENTRY_OFFSET: usize = 8;
        // const POLYMORPHIC_ENTRY_OFFSET: usize = 24;

        for idx in 0..cluster.non_deferred_obj_count
        // normal code objects
        {
            // --- ReadInstructions ---
            let payload_info = stream.read_unsigned()?;
            let has_monomorphic_entrypoint = (payload_info & 1) == 1;
            let unchecked_offset = payload_info >> 1;

            let obj = cluster.objs.get_mut(idx as usize).unwrap(); // this can never panic

            // unresolved before resolve_entrypoints
            obj.entry_point = u64::MAX; // [[IMPORTANT]] WE USE u64::MAX to indicate an unresolved entry
            obj.monomorphic_entry_point = u64::MAX; // so u64::MAX should NEVER be the final state for non-deferred Code objects
            obj.unchecked_entry_point = unchecked_offset;
            obj.monomorphic_unchecked_entry_point = unchecked_offset;
            obj.has_monomorphic_entrypoint = has_monomorphic_entrypoint; // used to resolve monomorphic_unchecked_entry_pint
                                                                         // --- ReadInstructions ---

            // really important, in FullAOT all code objects use the
            // global object pool, and DO NOT store a refid to any particular ObjectPool
            // even though there are multiple other cases like this if we compare
            // AOT vs JIT, this one is important enough to leave a piece of code
            // reminding us of it

            obj.object_pool = 0;
            obj.owner = stream.read_ref_id()?;
            obj.exception_handlers = stream.read_ref_id()?;
            obj.pc_descriptors = stream.read_ref_id()?;
            obj.catch_entry = stream.read_ref_id()?;

            // again, for AOT snapshots the stackmaps aren't written to the clustered
            // stream, they are in the instruction tables along with the pc offset
            // into the instruction image
            obj.compressed_stackmaps = 0;
            obj.inlined_id_to_function = stream.read_ref_id()?;
            obj.code_source_map = stream.read_ref_id()?;
        }

        for idx in cluster.non_deferred_obj_count..cluster.obj_count
        // deferred code objects
        {
            // No read instructions equivalent
            let obj = cluster.objs.get_mut(idx as usize).unwrap();

            // always unresolved inside the current snapshot, these
            // are resolved during runtime, at some point we would have to emulate the behavior of
            // the Unit serialization/deserialization process, i.e ReadUnitSnapshot
            // so for now, we do NOT support this
            obj.entry_point = u64::MAX;
            obj.monomorphic_entry_point = u64::MAX;
            obj.unchecked_entry_point = u64::MAX;
            obj.monomorphic_unchecked_entry_point = u64::MAX;

            obj.object_pool = 0;
            obj.owner = stream.read_ref_id()?;
            obj.exception_handlers = stream.read_ref_id()?;
            obj.pc_descriptors = stream.read_ref_id()?;
            obj.catch_entry = stream.read_ref_id()?;
            obj.compressed_stackmaps = 0;
            obj.inlined_id_to_function = stream.read_ref_id()?;
            obj.code_source_map = stream.read_ref_id()?;
        }
    }
);

// this is the analogous function to the sdk's Deserializer::EndInstructions
pub fn resolve_instructions_len_for_code_objects(
    clusters: &mut HashMap<u32, Box<dyn Cluster>>,
    instr_image_size: usize,
) -> anyhow::Result<()> {
    const POLYMORPHIC_ENTRY_OFFSET: u64 = 24;

    let code_cluster = clusters
        .get_mut(&((ClassId::CodeCid as u32) << 2))
        .ok_or_else(|| anyhow::anyhow!("Code cluster not found"))?
        .as_any_mut()
        .downcast_mut::<CodeCluster>()
        .ok_or_else(|| anyhow::anyhow!("Code cluster is not of type CodeCluster"))?;

    let mut curr_end = instr_image_size as u64;
    for idx in (0..code_cluster.non_deferred_obj_count).rev() {
        let code_obj = code_cluster.objs.get_mut(idx as usize).unwrap();
        let entry_offset = if code_obj.has_monomorphic_entrypoint {
            POLYMORPHIC_ENTRY_OFFSET
        } else {
            0
        };
        code_obj.instructions_length_ = curr_end - (code_obj.entry_point - entry_offset);
        curr_end = code_obj.entry_point - entry_offset;
    }

    Ok(())
}

pub fn resolve_entrypoints(
    // we use find_map for both clusters given there should only be a single Cluster for Function and Code objects.
    clusters: &mut HashMap<u32, Box<dyn Cluster>>,
    instruction_table: &InstructionTable,
    expected_non_deferred_code_count: usize,
) -> anyhow::Result<()> {
    const MONOMORPHIC_ENTRY_OFFSET: u64 = 8;
    const POLYMORPHIC_ENTRY_OFFSET: u64 = 24;

    let first_entry_with_code = instruction_table.first_entry_with_code();
    let table_non_deferred_code_count = instruction_table
        .len()
        .checked_sub(first_entry_with_code)
        .ok_or_else(|| anyhow::anyhow!("instruction-table first Code entry exceeds its length"))?;

    anyhow::ensure!( // idk if this should be assert
        table_non_deferred_code_count == expected_non_deferred_code_count,
        "instruction table contains {table_non_deferred_code_count} non-deferred Code entries, but the snapshot header declares {expected_non_deferred_code_count}"
    );

    let code_entrypoints = if let Some(code_cluster) = clusters
        .values_mut()
        .find_map(|cluster| cluster.as_any_mut().downcast_mut::<CodeCluster>())
    {
        let non_deferred_count = usize::try_from(code_cluster.non_deferred_obj_count)
            .map_err(|_| anyhow::anyhow!("non-deferred Code count does not fit in usize"))?;

        anyhow::ensure!(
            non_deferred_count == expected_non_deferred_code_count,
            "Code cluster contains {non_deferred_count} non-deferred objects, but the snapshot header declares {expected_non_deferred_code_count}"
        );

        anyhow::ensure!(
            code_cluster.objs.len()
                == usize::try_from(code_cluster.obj_count)
                    .map_err(|_| anyhow::anyhow!("Code object count does not fit in usize"))?,
            "Code cluster object count does not match its allocated objects"
        );

        for (cluster_index, code) in code_cluster
            .objs
            .iter_mut()
            .take(non_deferred_count)
            .enumerate()
        {
            anyhow::ensure!(
                code.entry_point == u64::MAX && code.monomorphic_entry_point == u64::MAX,
                "Code entry points were already resolved"
            );
            anyhow::ensure!(
                code.unchecked_entry_point == code.monomorphic_unchecked_entry_point,
                "Code unchecked-entry addends do not match"
            );

            let code_cluster_index = u32::try_from(cluster_index)
                .map_err(|_| anyhow::anyhow!("Code cluster index does not fit in u32"))?;
            let payload_start =
                get_pc_offset_from_code_cluster_index(code_cluster_index, instruction_table)?
                    as u64;
            let unchecked_offset = code.unchecked_entry_point;
            let entry_offset = if code.has_monomorphic_entrypoint {
                POLYMORPHIC_ENTRY_OFFSET
            } else {
                0
            };
            let monomorphic_entry_offset = if code.has_monomorphic_entrypoint {
                MONOMORPHIC_ENTRY_OFFSET
            } else {
                0
            };

            // these ok_or_else checks were added
            // for robust error checking
            // but really this should never happen in normal snapshots, as with many other checks

            let entry_point = payload_start
                .checked_add(entry_offset)
                .ok_or_else(|| anyhow::anyhow!("Code entry-point offset overflow"))?;
            let monomorphic_entry_point = payload_start
                .checked_add(monomorphic_entry_offset)
                .ok_or_else(|| anyhow::anyhow!("Code monomorphic entry-point offset overflow"))?;
            let unchecked_entry_point = entry_point
                .checked_add(unchecked_offset)
                .ok_or_else(|| anyhow::anyhow!("Code unchecked entry-point offset overflow"))?;
            let monomorphic_unchecked_entry_point = monomorphic_entry_point
                .checked_add(unchecked_offset)
                .ok_or_else(|| {
                    anyhow::anyhow!("Code momomorphic unchecked entry-point offset overflow")
                })?;

            code.entry_point = entry_point;
            code.monomorphic_entry_point = monomorphic_entry_point;
            code.unchecked_entry_point = unchecked_entry_point;
            code.monomorphic_unchecked_entry_point = monomorphic_unchecked_entry_point;
        }

        code_cluster
            .objs
            .iter()
            .map(|code| (code.entry_point, code.unchecked_entry_point))
            .collect::<Vec<_>>()
    } else {
        // this should never happen in a normal Flutter-generated snapshot
        anyhow::ensure!(
            expected_non_deferred_code_count == 0,
            "snapshot declares non-deferred Code objects but has no Code cluster"
        );
        Vec::new()
    };

    // this has to happen after the code cluster entry resolution
    // because this resolution is really just grabbing the code_index to find the
    // respective Code object and retrieve its fields

    if let Some(function_cluster) = clusters
        .values_mut()
        .find_map(|cluster| cluster.as_any_mut().downcast_mut::<FunctionCluster>())
    {
        for function in &mut function_cluster.objs {
            anyhow::ensure!(
                function.entry_point == u64::MAX && function.unchecked_entry_point == u64::MAX,
                "Function entry points were already resolved"
            );

            if function.code_index == 0 {
                continue;
            }

            let table_index = (function.code_index - 1) as usize;
            if table_index < first_entry_with_code {
                let entry_point = instruction_table.pc_offset_at(table_index)?;
                function.entry_point = entry_point;
                function.unchecked_entry_point = entry_point;
                continue;
            }

            // instead of iterating through the CodeCluster, we just return a vec of (entry, unchecked_entry)
            // for the Code objects we just resolved. Which is ordered of course, following the same order as the
            // objs array of CodeCluster.
            let code_cluster_index = table_index - first_entry_with_code;
            let (entry_point, unchecked_entry_point) = code_entrypoints
                .get(code_cluster_index)
                .copied()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Function code index {} maps past the Code cluster",
                        function.code_index
                    )
                })?;
            function.entry_point = entry_point;
            function.unchecked_entry_point = unchecked_entry_point;
        }
    }

    Ok(())
}
DECLARE_VARIABLE_LENGTH_CLUSTER!(ObjectPool, ObjectPoolCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    ObjectPoolCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            let length = usize::try_from(stream.read_unsigned()?)
                .map_err(|_| anyhow::anyhow!("ObjectPool length does not fit in usize"))?;
            cluster.objs.push(Box::new(ObjectPool {
                length,
                entries: Vec::new(),
            }));
        }
    },
    |cluster, stream| {
        const ENTRY_TYPE_MASK: u8 = 0x0f;
        const SNAPSHOT_BEHAVIOR_SHIFT: u32 = 5;
        const SNAPSHOT_BEHAVIOR_MASK: u8 = 0x07;

        const IMMEDIATE: u8 = 0;
        const TAGGED_OBJECT: u8 = 1;
        const NATIVE_FUNCTION: u8 = 2;

        const SNAPSHOTABLE: u8 = 0;
        const NOT_SNAPSHOTABLE: u8 = 1;
        const RESET_TO_BOOTSTRAP_NATIVE: u8 = 2;
        const RESET_TO_SWITCHABLE_CALL_MISS_ENTRY_POINT: u8 = 3;
        const SET_TO_ZERO: u8 = 4;

        for obj in &mut cluster.objs {
            let fill_length = usize::try_from(stream.read_unsigned()?)
                .map_err(|_| anyhow::anyhow!("ObjectPool fill length does not fit in usize"))?;
            anyhow::ensure!(
                fill_length == obj.length,
                "length changed between alloc ({}) and fill ({fill_length})",
                obj.length
            );

            for _ in 0..fill_length {
                let entry_bits = stream.read_byte()?;
                let entry_type = entry_bits & ENTRY_TYPE_MASK;
                let snapshot_behavior =
                    (entry_bits >> SNAPSHOT_BEHAVIOR_SHIFT) & SNAPSHOT_BEHAVIOR_MASK;

                let value = match snapshot_behavior {
                    SNAPSHOTABLE => match entry_type {
                        TAGGED_OBJECT => {
                            ObjectPoolEntryValue::TaggedObjectRef(stream.read_ref_id()?)
                        }
                        IMMEDIATE => ObjectPoolEntryValue::Immediate(stream.read()? as i64),
                        NATIVE_FUNCTION => ObjectPoolEntryValue::NativeFunctionLazyLink,
                        _ => anyhow::bail!(
                            "unsupported snapshotable ObjectPool entry type {entry_type}"
                        ),
                    },
                    NOT_SNAPSHOTABLE => {
                        anyhow::bail!("ObjectPool contains an entry marked as not snapshotable")
                    }
                    RESET_TO_BOOTSTRAP_NATIVE => ObjectPoolEntryValue::ResetToBootstrapNative,
                    RESET_TO_SWITCHABLE_CALL_MISS_ENTRY_POINT => {
                        ObjectPoolEntryValue::ResetToSwitchableCallMissEntryPoint
                    }
                    SET_TO_ZERO => ObjectPoolEntryValue::SetToZero,
                    _ => anyhow::bail!(
                        "unsupported ObjectPool snapshot behavior {snapshot_behavior}"
                    ),
                };

                obj.entries.push(ObjectPoolEntry { entry_bits, value });
            }
        }
    }
);

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

DECLARE_VARIABLE_LENGTH_CLUSTER!(TypedData, DeltaEncodedTypedDataCluster);
IMPLEMENT_VARIABLE_LENGTH_CLUSTER!(
    DeltaEncodedTypedDataCluster,
    |cluster, stream| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            let length_in_bytes = usize::try_from(stream.read_unsigned()?).map_err(|_| {
                anyhow::anyhow!("delta-encoded TypedData length does not fit usize")
            })?;
            cluster.objs.push(Box::new(TypedData {
                length: length_in_bytes,
                ..TypedData::default()
            }));
        }
    },
    |cluster, stream| {
        for obj in &mut cluster.objs {
            let allocated_length_in_bytes = obj.length;
            let encoded_length = stream.read_unsigned()?;
            let length = usize::try_from(encoded_length >> 1).map_err(|_| {
                anyhow::anyhow!("delta-encoded TypedData length does not fit usize")
            })?;
            let element_size = if encoded_length & 1 == 0 { 2 } else { 4 };
            let length_in_bytes = length
                .checked_mul(element_size)
                .ok_or_else(|| anyhow::anyhow!("delta-encoded TypedData byte length overflow"))?;
            anyhow::ensure!(
                length_in_bytes == allocated_length_in_bytes,
                "delta-encoded TypedData alloc/fill length mismatch"
            );

            obj.length = length;
            obj.data = Vec::with_capacity(length_in_bytes);
            let mut value = 0_u64;
            for _ in 0..length {
                value = value
                    .checked_add(stream.read_unsigned()?)
                    .ok_or_else(|| anyhow::anyhow!("delta-encoded TypedData value overflow"))?;
                if element_size == 2 {
                    let value = u16::try_from(value)
                        .map_err(|_| anyhow::anyhow!("delta-encoded Uint16 value overflow"))?;
                    obj.data.extend_from_slice(&value.to_le_bytes());
                } else {
                    let value = u32::try_from(value)
                        .map_err(|_| anyhow::anyhow!("delta-encoded Uint32 value overflow"))?;
                    obj.data.extend_from_slice(&value.to_le_bytes());
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
            read_canonical_set_layout(cluster.obj_count, stream)?;
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
        if cluster.is_canonical {
            read_canonical_set_layout(cluster.obj_count, stream)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::UNSIGNED_M;

    fn leb_unsigned(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let low = (value & 0x7f) as u8;
            let rest = value >> 7;
            if rest == 0 && low <= (0xff - UNSIGNED_M) {
                out.push(low + UNSIGNED_M);
                return out;
            }
            out.push(low);
            value = rest;
        }
    }

    /// StringDeserializationCluster writes `length << 1 | is_two_byte` in the
    /// alloc section and again in the fill section, then the payload.
    fn encode_string_cluster(strings: &[(&str, bool)]) -> Vec<u8> {
        let encoded = |s: &str, two: bool| -> u64 {
            let len = if two {
                s.chars().map(|c| c.len_utf16()).sum()
            } else {
                s.chars().count()
            };
            ((len as u64) << 1) | two as u64
        };
        let mut alloc = leb_unsigned(strings.len() as u64);
        for (s, two) in strings {
            alloc.extend_from_slice(&leb_unsigned(encoded(s, *two)));
        }
        let mut fill = Vec::new();
        for (s, two) in strings {
            fill.extend_from_slice(&leb_unsigned(encoded(s, *two)));
            if *two {
                for u in s.encode_utf16() {
                    fill.extend_from_slice(&u.to_le_bytes());
                }
            } else {
                fill.extend(s.chars().map(|c| c as u32 as u8));
            }
        }
        alloc.extend(fill);
        alloc
    }

    fn read_strings(bytes: &[u8], count: usize) -> Vec<String> {
        let mut cluster = _StringCluster::default();
        let mut stream = Stream::new(bytes);
        let mut next_ref = 1u64;
        cluster.read_alloc(&mut next_ref, &mut stream).unwrap();
        cluster.read_fill(&mut stream).unwrap();
        assert_eq!(
            next_ref,
            1 + count as u64,
            "alloc must claim one ref per string"
        );
        cluster
            .objs
            .iter()
            .map(|o| o.internal_str.clone())
            .collect()
    }

    #[test]
    fn one_byte_strings_round_trip() {
        let input = [
            ("main", false),
            ("package:app/main.dart", false),
            ("", false),
        ];
        let bytes = encode_string_cluster(&input);
        assert_eq!(
            read_strings(&bytes, input.len()),
            vec!["main", "package:app/main.dart", ""]
        );
    }

    /// The payload is Latin-1, so bytes above 0x7F are ordinary characters.
    /// Decoding as UTF-8 used to panic the whole parse here.
    #[test]
    fn one_byte_strings_carry_latin1_payloads() {
        let input = [("caf\u{e9}", false), ("\u{ff}\u{80}", false)];
        let bytes = encode_string_cluster(&input);
        assert_eq!(
            read_strings(&bytes, input.len()),
            vec!["caf\u{e9}", "\u{ff}\u{80}"]
        );
    }

    #[test]
    fn two_byte_strings_round_trip() {
        let input = [("\u{4f60}\u{597d}", true), ("mixed \u{2713}", true)];
        let bytes = encode_string_cluster(&input);
        assert_eq!(
            read_strings(&bytes, input.len()),
            vec!["\u{4f60}\u{597d}", "mixed \u{2713}"]
        );
    }

    /// Both widths in one cluster: the per-object type comes from the low bit
    /// of the encoded word, so mixing them catches a mis-shifted length.
    #[test]
    fn mixed_width_cluster_stays_in_sync() {
        let input = [
            ("ascii", false),
            ("\u{4f60}", true),
            ("caf\u{e9}", false),
            ("\u{2713}", true),
        ];
        let bytes = encode_string_cluster(&input);
        assert_eq!(
            read_strings(&bytes, input.len()),
            vec!["ascii", "\u{4f60}", "caf\u{e9}", "\u{2713}"]
        );
    }

    /// Dart strings may hold unpaired surrogates, which are not valid UTF-16.
    /// Those must decode lossily rather than panic.
    #[test]
    fn unpaired_surrogates_do_not_panic() {
        let mut bytes = leb_unsigned(1);
        bytes.extend_from_slice(&leb_unsigned((1 << 1) | 1)); // one two-byte unit
        bytes.extend_from_slice(&leb_unsigned((1 << 1) | 1)); // repeated in the fill section
        bytes.extend_from_slice(&0xd800u16.to_le_bytes()); // lone high surrogate
        assert_eq!(read_strings(&bytes, 1), vec!["\u{fffd}"]);
    }

    #[test]
    fn a_truncated_payload_errors_instead_of_panicking() {
        let mut bytes = encode_string_cluster(&[("hello", false)]);
        bytes.truncate(bytes.len() - 3);
        let mut cluster = _StringCluster::default();
        let mut stream = Stream::new(&bytes);
        let mut next_ref = 1u64;
        cluster.read_alloc(&mut next_ref, &mut stream).unwrap();
        assert!(cluster.read_fill(&mut stream).is_err());
    }
}
