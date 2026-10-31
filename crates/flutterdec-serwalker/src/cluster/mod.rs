mod base_objects_pseudocluster;

use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::{any, u64};

use crate::constants::{Cid, ClassKind};
use crate::instruction_table::{get_pc_offset_from_code_cluster_index, InstructionTable};
use crate::profile_reader::profile::{DataProfile, ProfileOperation, TargetType};
use crate::profile_reader::profile_json_dictionary::*;
use crate::raw_object::object_vocabulary::CoreObject;
use crate::raw_object::*;
use crate::stream::Stream;
use crate::DECLARE_FIXED_LENGTH_CLUSTER;
use crate::DECLARE_VARIABLE_LENGTH_CLUSTER;

pub trait Cluster {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
    fn as_any(&self) -> &dyn std::any::Any;
    fn object_by_ref_id(&self, id: u32) -> Option<&dyn std::any::Any>;
    fn cid(&self) -> Cid;
    fn first_ref_id(&self) -> u32;
    fn set_metadata(&mut self, tags: u32, cid: Cid, is_immutable: bool, is_canonical: bool);
    fn is_fixed_len(&self) -> bool;
    fn read_alloc(
        &mut self,
        last_ref_id: &mut u64,
        stream: &mut Stream,
        profile: &DataProfile,
    ) -> anyhow::Result<usize>;
    fn read_fill(&mut self, stream: &mut Stream, profile: &DataProfile) -> anyhow::Result<usize>;
}

pub fn read_smi(stream: &mut Stream) -> anyhow::Result<Smi> {
    let raw_smi = stream.read()?; // smis are always written as signed numbers

    Ok(raw_smi as Smi)
}

fn is_typed_data_kind(kind: ClassKind, remainder: u32) -> bool {
    let first = ClassKind::TypedDataInt8Array as u32;
    let last = ClassKind::UnmodifiableTypedDataFloat64x2ArrayView as u32;
    let kind_number = kind as u32;
    (first..=last).contains(&kind_number) && (kind_number - first) % 4 == remainder
}

pub fn decide_cluster(kind: ClassKind) -> Result<Box<dyn Cluster>, &'static str> {
    if kind == ClassKind::NativePointer {
        return Ok(Box::new(DeltaEncodedTypedDataCluster::default()));
    }
    if kind == ClassKind::Instance {
        return Ok(Box::new(InstanceCluster::default()));
    }
    if is_typed_data_kind(kind, 1) || kind == ClassKind::ByteDataView {
        return Ok(Box::new(TypedDataViewCluster::default()));
    }
    if is_typed_data_kind(kind, 2) {
        return Ok(Box::new(ExternalTypedDataCluster::default()));
    }
    if is_typed_data_kind(kind, 0) {
        return Ok(Box::new(TypedDataCluster::default()));
    }

    match kind {
        ClassKind::Class => Ok(Box::new(ClassCluster::default())),
        ClassKind::TypeParameters => Ok(Box::new(TypeParametersCluster::default())),
        ClassKind::TypeArguments => Ok(Box::new(TypeArgumentsCluster::default())),
        ClassKind::PatchClass => Ok(Box::new(PatchClassCluster::default())),
        ClassKind::Function => Ok(Box::new(FunctionCluster::default())),
        ClassKind::ClosureData => Ok(Box::new(ClosureDataCluster::default())),
        ClassKind::FfiTrampolineData => Ok(Box::new(FfiTrampolineDataCluster::default())),
        ClassKind::Field => Ok(Box::new(FieldCluster::default())),
        ClassKind::Script => Ok(Box::new(ScriptCluster::default())),
        ClassKind::Library => Ok(Box::new(LibraryCluster::default())),
        ClassKind::Namespace => Ok(Box::new(NamespaceCluster::default())),
        ClassKind::Code => Ok(Box::new(CodeCluster::default())),
        ClassKind::ObjectPool => Ok(Box::new(ObjectPoolCluster::default())),
        ClassKind::PcDescriptors => Ok(Box::new(PcDescriptorsCluster::default())),
        ClassKind::CodeSourceMap => Ok(Box::new(CodeSourceMapCluster::default())),
        ClassKind::CompressedStackMaps => Ok(Box::new(CompressedStackMapsCluster::default())),
        ClassKind::ExceptionHandlers => Ok(Box::new(ExceptionHandlersCluster::default())),
        ClassKind::Context => Ok(Box::new(ContextCluster::default())),
        ClassKind::ContextScope => Ok(Box::new(ContextScopeCluster::default())),
        ClassKind::UnlinkedCall => Ok(Box::new(UnlinkedCallCluster::default())),
        ClassKind::ICData => Ok(Box::new(ICDataCluster::default())),
        ClassKind::MegamorphicCache => Ok(Box::new(MegamorphicCacheCluster::default())),
        ClassKind::SubtypeTestCache => Ok(Box::new(SubtypeTestCacheCluster::default())),
        ClassKind::LoadingUnit => Ok(Box::new(LoadingUnitCluster::default())),
        ClassKind::LanguageError => Ok(Box::new(LanguageErrorCluster::default())),
        ClassKind::UnhandledException => Ok(Box::new(UnhandledExceptionCluster::default())),
        ClassKind::LibraryPrefix => Ok(Box::new(LibraryPrefixCluster::default())),
        ClassKind::Type => Ok(Box::new(TypeCluster::default())),
        ClassKind::FunctionType => Ok(Box::new(FunctionTypeCluster::default())),
        ClassKind::RecordType => Ok(Box::new(RecordTypeCluster::default())),
        ClassKind::TypeParameter => Ok(Box::new(TypeParameterCluster::default())),
        ClassKind::Closure => Ok(Box::new(ClosureCluster::default())),
        ClassKind::Mint => Ok(Box::new(MintCluster::default())),
        ClassKind::Double => Ok(Box::new(DoubleCluster::default())),
        ClassKind::Int32x4 => Ok(Box::new(Int32x4Cluster::default())),
        ClassKind::Float32x4 => Ok(Box::new(Float32x4Cluster::default())),
        ClassKind::Float64x2 => Ok(Box::new(Float64x2Cluster::default())),
        ClassKind::GrowableObjectArray => Ok(Box::new(GrowableObjectArrayCluster::default())),
        ClassKind::Record => Ok(Box::new(RecordCluster::default())),
        ClassKind::StackTrace => Ok(Box::new(StackTraceCluster::default())),
        ClassKind::RegExp => Ok(Box::new(RegExpCluster::default())),
        ClassKind::WeakProperty => Ok(Box::new(WeakPropertyCluster::default())),
        ClassKind::ConstMap => Ok(Box::new(ConstMapCluster::default())),
        ClassKind::ConstSet => Ok(Box::new(ConstSetCluster::default())),
        ClassKind::Array => Ok(Box::new(ArrayCluster::default())),
        ClassKind::ImmutableArray => Ok(Box::new(ImmutableArrayCluster::default())),
        ClassKind::WeakArray => Ok(Box::new(WeakArrayCluster::default())),
        ClassKind::_String => Ok(Box::new(_StringCluster::default())),
        ClassKind::FfiInt8
        | ClassKind::FfiInt16
        | ClassKind::FfiInt32
        | ClassKind::FfiInt64
        | ClassKind::FfiUint8
        | ClassKind::FfiUint16
        | ClassKind::FfiUint32
        | ClassKind::FfiUint64
        | ClassKind::FfiFloat
        | ClassKind::FfiDouble
        | ClassKind::FfiVoid
        | ClassKind::FfiHandle
        | ClassKind::FfiBool => Ok(Box::new(InstanceCluster::default())),
        ClassKind::Illegal => Err("illegal CID cannot identify a serialized cluster"),
        ClassKind::Map | ClassKind::Set => {
            Err("mutable Map and Set clusters are unreachable in snapshots")
        }
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
        // We do not really care about the canonical objects layout, so just read and discard
        let _gap = stream.read_unsigned()?;
    }

    Ok(())
}

struct ObjectRecipe<F> {
    operations: Vec<ProfileOperation<F>>,
    handler: Option<String>,
    canonical_set_layout: bool,
}

fn object_recipe<T: CoreObject>(
    profile: &DataProfile,
    name: &str,
    phase: &str,
) -> anyhow::Result<ObjectRecipe<T::Field>> {
    use anyhow::Context;

    let recipe = profile
        .parsing_operations
        .get(name)
        .and_then(|cluster| cluster.get(phase))
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| anyhow::anyhow!("profile has no {name}.{phase} recipe"))?;
    let operations = recipe
        .get(FOR_EACH)
        .ok_or_else(|| anyhow::anyhow!("profile recipe {name}.{phase} has no {FOR_EACH}"))?;
    let operations = serde_json::from_value(operations.clone())
        .with_context(|| format!("decode {name}.{phase}.{FOR_EACH}"))?;
    let handler = recipe
        .get(HANDLER)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| anyhow::anyhow!("{name}.{phase}.{HANDLER} must be a string"))
        })
        .transpose()?;
    let canonical_set_layout = recipe
        .get(CANONICAL_SET_LAYOUT)
        .map(|value| {
            value.as_bool().ok_or_else(|| {
                anyhow::anyhow!("{name}.{phase}.{CANONICAL_SET_LAYOUT} must be a boolean")
            })
        })
        .transpose()?
        .unwrap_or(false);

    Ok(ObjectRecipe {
        operations,
        handler,
        canonical_set_layout,
    })
}

fn property_count(value: &PropType) -> anyhow::Result<usize> {
    let count = match value {
        PropType::Byte(value) => u64::from(*value),
        PropType::RefId(value) | PropType::RawU32(value) => u64::from(*value),
        PropType::Unsigned(value) | PropType::RawU64(value) => *value,
        PropType::Signed(value) => {
            u64::try_from(*value).map_err(|_| anyhow::anyhow!("negative array length {value}"))?
        }
        _ => anyhow::bail!("array length is not an integer"),
    };
    Ok(usize::try_from(count)?)
}

fn array_count<T: CoreObject>(
    object: &T,
    context: &HashMap<String, PropType>,
    length_from: &str,
    multiplier: usize,
) -> anyhow::Result<usize> {
    let count = if let Some(value) = context.get(length_from) {
        property_count(value)?
    } else {
        let field =
            serde_json::from_value::<T::Field>(serde_json::Value::String(length_from.into()))?;
        let value = object.core_field(field);
        if let Some(value) = value.downcast_ref::<usize>() {
            *value
        } else if let Some(value) = value.downcast_ref::<u64>() {
            usize::try_from(*value)?
        } else if let Some(value) = value.downcast_ref::<u32>() {
            usize::try_from(*value)?
        } else if let Some(value) = value.downcast_ref::<u16>() {
            usize::from(*value)
        } else if let Some(value) = value.downcast_ref::<i32>() {
            usize::try_from(*value)?
        } else if let Some(value) = value.downcast_ref::<i64>() {
            usize::try_from(*value)?
        } else {
            anyhow::bail!("{length_from} is not an array-length field")
        }
    };
    count
        .checked_mul(multiplier)
        .ok_or_else(|| anyhow::anyhow!("array length multiplication overflow"))
}

fn core_byte_length<T: CoreObject + 'static>(
    object: &T,
    cid: Cid,
    profile: &DataProfile,
    stream: &mut Stream,
) -> anyhow::Result<usize> {
    let object = object as &dyn any::Any;
    if object.is::<Int32x4>() || object.is::<Float32x4>() || object.is::<Float64x2>() {
        return Ok(16);
    }
    let length = if let Some(object) = object.downcast_ref::<TypedData>() {
        Some(object.length)
    } else if let Some(object) = object.downcast_ref::<ExternalTypedData>() {
        stream.align_stream(8)?;
        Some(object.length)
    } else {
        None
    };
    if let Some(length) = length {
        return length
            .checked_mul(typed_data_element_size(cid, profile)?)
            .ok_or_else(|| anyhow::anyhow!("typed-data byte length overflow"));
    }
    if let Some(object) = object.downcast_ref::<CodeSourceMap>() {
        return Ok(usize::try_from(object.length)?);
    }
    if let Some(object) = object.downcast_ref::<PcDescriptors>() {
        return Ok(usize::try_from(object.length)?);
    }
    if let Some(object) = object.downcast_ref::<CompressedStackMaps>() {
        return Ok(usize::try_from(object.length)?);
    }
    anyhow::bail!("{} has no byte-payload handler", std::any::type_name::<T>())
}

fn read_primitive(codec: &TargetType, stream: &mut Stream) -> anyhow::Result<PropType> {
    Ok(match codec {
        TargetType::RefId => PropType::RefId(stream.read_ref_id()?),
        TargetType::Unsigned => PropType::Unsigned(stream.read_unsigned()?),
        TargetType::Signed => PropType::Signed(stream.read()? as i64),
        TargetType::Byte => PropType::Byte(stream.read_byte()?),
        TargetType::RawU32 => PropType::RawU32(stream.read_raw_u32()?),
        TargetType::RawU64 => PropType::RawU64(stream.read_raw_u64()?),
        TargetType::Bytes => anyhow::bail!("bytes require a Core byte-payload handler"),
        TargetType::Array { .. } => anyhow::bail!("nested array requires an array-length context"),
    })
}

fn primitive_array(
    operations: &[ProfileOperation<String>],
    count: usize,
    stream: &mut Stream,
) -> anyhow::Result<Vec<PropType>> {
    let mut values = Vec::new();
    for _ in 0..count {
        values.push(
            primitive_element(operations, stream)?
                .ok_or_else(|| anyhow::anyhow!("array element recipe has no value"))?,
        );
    }
    Ok(values)
}

fn primitive_element(
    operations: &[ProfileOperation<String>],
    stream: &mut Stream,
) -> anyhow::Result<Option<PropType>> {
    let mut element = None;
    for operation in operations {
        match operation {
            ProfileOperation::Core { target, codec } if target == "value" => {
                anyhow::ensure!(
                    element.is_none(),
                    "array element has multiple value operations"
                );
                element = Some(read_primitive(codec, stream)?);
            }
            ProfileOperation::Discard { codec } => {
                read_primitive(codec, stream)?;
            }
            _ => {
                anyhow::bail!("primitive array recipe requires a Core value and optional discards")
            }
        }
    }
    Ok(element)
}

fn read_core_array<T: CoreObject + 'static>(
    object: &mut T,
    target: T::Field,
    count: usize,
    for_each: &[serde_json::Value],
    stream: &mut Stream,
    cid: Cid,
    profile: &DataProfile,
) -> anyhow::Result<()> {
    use crate::raw_object::object_vocabulary::ExceptionHandlerInfoField;

    let field = object.core_field_mut(target);
    if let Some(elements) = field.downcast_mut::<Vec<u32>>() {
        let operations = serde_json::from_value::<Vec<ProfileOperation<String>>>(
            serde_json::Value::Array(for_each.to_vec()),
        )?;
        elements.clear();
        for _ in 0..count {
            match primitive_element(&operations, stream)? {
                Some(PropType::RefId(id)) => elements.push(id),
                _ => anyhow::bail!("Core reference-array element must use ref_id"),
            }
        }
        return Ok(());
    }
    if let Some(elements) = field.downcast_mut::<Vec<ExceptionHandlerInfo>>() {
        let operations = serde_json::from_value::<Vec<ProfileOperation<ExceptionHandlerInfoField>>>(
            serde_json::Value::Array(for_each.to_vec()),
        )?;
        elements.clear();
        for _ in 0..count {
            let mut element = ExceptionHandlerInfo::default();
            execute_object_recipe(&mut element, &operations, stream, cid, profile)?;
            elements.push(element);
        }
        return Ok(());
    }
    anyhow::bail!("Core field {target:?} has no typed array handler")
}

fn execute_object_recipe<T: CoreObject + 'static>(
    object: &mut T,
    operations: &[ProfileOperation<T::Field>],
    stream: &mut Stream,
    cid: Cid,
    profile: &DataProfile,
) -> anyhow::Result<()> {
    use anyhow::Context;

    let mut context = HashMap::new();
    for (operation_index, operation) in operations.iter().enumerate() {
        let result = (|| {
            match operation {
                ProfileOperation::Core { target, codec } => match codec {
                    TargetType::Array {
                        length_from,
                        multiplier,
                        for_each,
                    } => {
                        let count = array_count(object, &context, length_from, *multiplier)?;
                        read_core_array(object, *target, count, for_each, stream, cid, profile)?;
                    }
                    TargetType::Bytes => {
                        let length = core_byte_length(object, cid, profile, stream)?;
                        object.set_core_field(
                            *target,
                            PropType::Bytes(stream.read_bytes(length)?),
                        )?;
                    }
                    _ => object.set_core_field(*target, read_primitive(codec, stream)?)?,
                },
                ProfileOperation::Extra { target, codec } => {
                    let value = if let TargetType::Array {
                        length_from,
                        multiplier,
                        for_each,
                    } = codec
                    {
                        let count = array_count(object, &context, length_from, *multiplier)?;
                        let operations = serde_json::from_value::<Vec<ProfileOperation<String>>>(
                            serde_json::Value::Array(for_each.clone()),
                        )?;
                        PropType::Array(primitive_array(&operations, count, stream)?)
                    } else {
                        read_primitive(codec, stream)?
                    };
                    object.extra_properties_mut().insert(target.clone(), value);
                }
                ProfileOperation::Context { slot, codec } => {
                    context.insert(slot.clone(), read_primitive(codec, stream)?);
                }
                ProfileOperation::Discard { codec } => {
                    if let TargetType::Array {
                        length_from,
                        multiplier,
                        for_each,
                    } = codec
                    {
                        let count = array_count(object, &context, length_from, *multiplier)?;
                        let operations = serde_json::from_value::<Vec<ProfileOperation<String>>>(
                            serde_json::Value::Array(for_each.clone()),
                        )?;
                        for _ in 0..count {
                            primitive_element(&operations, stream)?;
                        }
                    } else {
                        read_primitive(codec, stream)?;
                    }
                }
            }
            Ok::<_, anyhow::Error>(())
        })();
        result.with_context(|| {
            format!(
                "{} operation {operation_index} at stream offset {}",
                T::PROFILE_NAME,
                stream.get_current_pos()
            )
        })?;
    }
    Ok(())
}

fn execute_alloc_profile<T: CoreObject + Default + 'static>(
    objects: &mut Vec<Box<T>>,
    recipe: &ObjectRecipe<T::Field>,
    stream: &mut Stream,
    cid: Cid,
    canonical: bool,
    profile: &DataProfile,
) -> anyhow::Result<u64> {
    anyhow::ensure!(
        recipe.handler.is_none(),
        "custom allocation handler was not dispatched"
    );
    let count = stream.read_unsigned()?;
    for _ in 0..count {
        let mut object = Box::<T>::default();
        execute_object_recipe(object.as_mut(), &recipe.operations, stream, cid, profile)?;
        objects.push(object);
    }
    if canonical && recipe.canonical_set_layout {
        read_canonical_set_layout(count, stream)?;
    }
    Ok(count)
}

fn execute_fill_profile<T: CoreObject + 'static>(
    objects: &mut [Box<T>],
    recipe: &ObjectRecipe<T::Field>,
    stream: &mut Stream,
    cid: Cid,
    profile: &DataProfile,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        recipe.handler.is_none(),
        "custom fill handler was not dispatched"
    );
    for object in objects {
        if let Some(function) = (object.as_mut() as &mut dyn any::Any).downcast_mut::<Function>() {
            function.entry_point = u64::MAX;
            function.unchecked_entry_point = u64::MAX;
        }
        execute_object_recipe(object.as_mut(), &recipe.operations, stream, cid, profile)?;
    }
    Ok(())
}

#[macro_export]
macro_rules! IMPLEMENT_PROFILED_CLUSTER {
    ($name:ident, $cluster_name:ident, $fixed:literal) => {
        impl Cluster for $cluster_name {
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn object_by_ref_id(&self, id: u32) -> Option<&dyn std::any::Any> {
                let index = id.checked_sub(self.first_ref_id)? as usize;
                self.objs
                    .get(index)
                    .map(|object| object.as_ref() as &dyn std::any::Any)
            }
            fn cid(&self) -> Cid {
                self.cid
            }
            fn first_ref_id(&self) -> u32 {
                self.first_ref_id
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
                $fixed
            }
            fn read_alloc(
                &mut self,
                last_ref_id: &mut u64,
                stream: &mut Stream,
                profile: &DataProfile,
            ) -> anyhow::Result<usize> {
                self.start_of_alloc = stream.get_current_pos();
                self.first_ref_id = u32::try_from(*last_ref_id)?;
                let recipe = object_recipe::<$name>(
                    profile,
                    <$name as CoreObject>::PROFILE_NAME,
                    READ_ALLOC,
                )?;
                self.obj_count = execute_alloc_profile(
                    &mut self.objs,
                    &recipe,
                    stream,
                    self.cid,
                    self.is_canonical,
                    profile,
                )?;
                *last_ref_id = last_ref_id
                    .checked_add(self.obj_count)
                    .ok_or_else(|| anyhow::anyhow!("reference-ID count overflow"))?;
                self.end_of_alloc = stream.get_current_pos();
                Ok(self.end_of_alloc - self.start_of_alloc)
            }
            fn read_fill(
                &mut self,
                stream: &mut Stream,
                profile: &DataProfile,
            ) -> anyhow::Result<usize> {
                self.start_of_fill = stream.get_current_pos();
                let recipe = object_recipe::<$name>(
                    profile,
                    <$name as CoreObject>::PROFILE_NAME,
                    READ_FILL,
                )?;
                execute_fill_profile(&mut self.objs, &recipe, stream, self.cid, profile)?;
                self.end_of_fill = stream.get_current_pos();
                Ok(self.end_of_fill - self.start_of_fill)
            }
        }
    };
}

// These are the objects that call ReadAllocFixedSize during deserialization,
// whose fill cluster size is uniquely determined by sizeof(Object) * num_of_objects
// and alloc cluster size is tags (MULEB128) + num_of_objects (MULEB128)

DECLARE_FIXED_LENGTH_CLUSTER!(TypeParameters, TypeParametersCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(PatchClass, PatchClassCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(Function, FunctionCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(ClosureData, ClosureDataCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(FfiTrampolineData, FfiTrampolineDataCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(Field, FieldCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(Script, ScriptCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(Library, LibraryCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(Namespace, NamespaceCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(KernelProgramInfo, KernelProgramInfoCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(UnlinkedCall, UnlinkedCallCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(ICData, ICDataCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(MegamorphicCache, MegamorphicCacheCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(SubtypeTestCache, SubtypeTestCacheCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(LoadingUnit, LoadingUnitCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(LanguageError, LanguageErrorCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(UnhandledException, UnhandledExceptionCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(LibraryPrefix, LibraryPrefixCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(Type, TypeCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(FunctionType, FunctionTypeCluster);
// obj.shape = stream.read_ref_id()?; as i32;
DECLARE_FIXED_LENGTH_CLUSTER!(RecordType, RecordTypeCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(TypeParameter, TypeParameterCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(Closure, ClosureCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(Double, DoubleCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(Int32x4, Int32x4Cluster);
DECLARE_FIXED_LENGTH_CLUSTER!(GrowableObjectArray, GrowableObjectArrayCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(TypedDataView, TypedDataViewCluster);
DECLARE_FIXED_LENGTH_CLUSTER!(ExternalTypedData, ExternalTypedDataCluster);
// obj.expand_inlined = stream.read_unsigned()? != 0; [[NOT PRESENT IN FullAOT]]
DECLARE_FIXED_LENGTH_CLUSTER!(StackTrace, StackTraceCluster);
// Deserializer::Read<int8_t>() dispatches to ReadStream::Raw<1, T>.
DECLARE_FIXED_LENGTH_CLUSTER!(RegExp, RegExpCluster);
// obj.next_seen_by_gc = stream.read_ref_id()?; [[NOT PRESENT IN FullAOT]]
DECLARE_FIXED_LENGTH_CLUSTER!(WeakProperty, WeakPropertyCluster);

macro_rules! IMPLEMENT_CUSTOM_PROFILED_CLUSTER {
    (
        $name:ident,
        $recipe_name:path,
        $cluster_name:ident,
        |$alloc_self:ident, $alloc_stream:ident, $alloc_profile:ident, $alloc_recipe:ident| $alloc_impl:block,
        |$fill_self:ident, $fill_stream:ident, $fill_profile:ident, $fill_recipe:ident| $fill_impl:block
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

            fn cid(&self) -> Cid {
                self.cid
            }

            fn first_ref_id(&self) -> u32 {
                self.first_ref_id
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
                _profile: &DataProfile,
            ) -> anyhow::Result<usize> {
                self.start_of_alloc = stream.get_current_pos();
                self.first_ref_id = u32::try_from(*last_ref_id)?;

                let recipe = object_recipe::<$name>(_profile, $recipe_name, READ_ALLOC)?;

                let $alloc_self = &mut *self;
                let $alloc_stream = &mut *stream;
                let $alloc_profile = _profile;
                let $alloc_recipe = &recipe;
                if let Some(handler) = recipe.handler.as_deref() {
                    anyhow::ensure!(
                        handler == $recipe_name,
                        "unexpected custom allocation handler {handler:?}"
                    );
                    anyhow::ensure!(
                        recipe.operations.is_empty() || $recipe_name == CODE,
                        "custom allocation recipe cannot ignore for_each operations"
                    );
                    $alloc_impl
                } else {
                    $alloc_self.obj_count = execute_alloc_profile(
                        &mut $alloc_self.objs,
                        &recipe,
                        $alloc_stream,
                        $alloc_self.cid,
                        $alloc_self.is_canonical,
                        _profile,
                    )?;
                }

                *last_ref_id = last_ref_id
                    .checked_add($alloc_self.obj_count)
                    .ok_or_else(|| anyhow::anyhow!("reference-ID count overflow"))?;
                $alloc_self.end_of_alloc = $alloc_stream.get_current_pos();
                Ok($alloc_self.end_of_alloc - $alloc_self.start_of_alloc)
            }

            fn read_fill(
                &mut self,
                stream: &mut Stream,
                _profile: &DataProfile,
            ) -> anyhow::Result<usize> {
                self.start_of_fill = stream.get_current_pos();

                let recipe = object_recipe::<$name>(_profile, $recipe_name, READ_FILL)?;

                let $fill_self = &mut *self;
                let $fill_stream = &mut *stream;
                let $fill_profile = _profile;
                let $fill_recipe = &recipe;
                if let Some(handler) = recipe.handler.as_deref() {
                    anyhow::ensure!(
                        handler == $recipe_name,
                        "unexpected custom fill handler {handler:?}"
                    );
                    anyhow::ensure!(
                        recipe.operations.is_empty()
                            || $recipe_name == CODE
                            || $recipe_name == CLASS,
                        "custom fill recipe cannot ignore for_each operations"
                    );
                    $fill_impl
                } else {
                    execute_fill_profile(
                        &mut $fill_self.objs,
                        &recipe,
                        $fill_stream,
                        $fill_self.cid,
                        _profile,
                    )?;
                }

                $fill_self.end_of_fill = $fill_stream.get_current_pos();
                Ok($fill_self.end_of_fill - $fill_self.start_of_fill)
            }
        }
    };
}

fn typed_data_element_size(cid: Cid, profile: &DataProfile) -> anyhow::Result<usize> {
    let class_kind = profile
        .cid_map
        .get(&cid)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("profile has no class kind for TypedData CID {cid}"))?;
    let size = match class_kind {
        ClassKind::TypedDataInt8Array
        | ClassKind::TypedDataUint8Array
        | ClassKind::TypedDataUint8ClampedArray
        | ClassKind::ExternalTypedDataInt8Array
        | ClassKind::ExternalTypedDataUint8Array
        | ClassKind::ExternalTypedDataUint8ClampedArray => 1,
        ClassKind::TypedDataInt16Array
        | ClassKind::TypedDataUint16Array
        | ClassKind::ExternalTypedDataInt16Array
        | ClassKind::ExternalTypedDataUint16Array => 2,
        ClassKind::TypedDataInt32Array
        | ClassKind::TypedDataUint32Array
        | ClassKind::TypedDataFloat32Array
        | ClassKind::ExternalTypedDataInt32Array
        | ClassKind::ExternalTypedDataUint32Array
        | ClassKind::ExternalTypedDataFloat32Array => 4,
        ClassKind::TypedDataInt64Array
        | ClassKind::TypedDataUint64Array
        | ClassKind::TypedDataFloat64Array
        | ClassKind::ExternalTypedDataInt64Array
        | ClassKind::ExternalTypedDataUint64Array
        | ClassKind::ExternalTypedDataFloat64Array => 8,
        ClassKind::TypedDataFloat32x4Array
        | ClassKind::TypedDataInt32x4Array
        | ClassKind::TypedDataFloat64x2Array
        | ClassKind::ExternalTypedDataFloat32x4Array
        | ClassKind::ExternalTypedDataInt32x4Array
        | ClassKind::ExternalTypedDataFloat64x2Array => 16,
        _ => anyhow::bail!("class {:?} is not a supported TypedData class", class_kind),
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

IMPLEMENT_CUSTOM_PROFILED_CLUSTER!(
    Code,
    CODE,
    CodeCluster,
    |cluster, stream, profile, recipe| {
        cluster.non_deferred_obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.non_deferred_obj_count {
            // for normal code objects
            let mut code = Box::<Code>::default();
            execute_object_recipe(
                code.as_mut(),
                &recipe.operations,
                stream,
                cluster.cid,
                profile,
            )?;
            cluster.objs.push(code);
        }

        cluster.deferred_obj_count = stream.read_unsigned()?; // number of deferred objs
        for _ in 0..cluster.deferred_obj_count {
            // for deferred code objects
            let mut code = Box::<Code>::default();
            execute_object_recipe(
                code.as_mut(),
                &recipe.operations,
                stream,
                cluster.cid,
                profile,
            )?;
            cluster.objs.push(code);
        }

        cluster.obj_count = cluster.non_deferred_obj_count + cluster.deferred_obj_count;
    },
    |cluster, stream, profile, recipe| {
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

            // again, for AOT snapshots the stackmaps aren't written to the clustered
            // stream, they are in the instruction tables along with the pc offset
            // into the instruction image
            obj.compressed_stackmaps = 0;
            execute_object_recipe(
                obj.as_mut(),
                &recipe.operations,
                stream,
                cluster.cid,
                profile,
            )?;
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
            obj.compressed_stackmaps = 0;
            execute_object_recipe(
                obj.as_mut(),
                &recipe.operations,
                stream,
                cluster.cid,
                profile,
            )?;
        }
    }
);

// this is the analogous function to the sdk's Deserializer::EndInstructions
pub fn resolve_instructions_len_for_code_objects(
    code_cluster: &mut CodeCluster,
    instr_image_size: usize,
) -> anyhow::Result<()> {
    const POLYMORPHIC_ENTRY_OFFSET: u64 = 24;

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
IMPLEMENT_CUSTOM_PROFILED_CLUSTER!(
    ObjectPool,
    OBJECT_POOL,
    ObjectPoolCluster,
    |cluster, stream, _profile, _recipe| {
        cluster.obj_count = stream.read_unsigned()?;
        for _ in 0..cluster.obj_count {
            let length = usize::try_from(stream.read_unsigned()?)
                .map_err(|_| anyhow::anyhow!("ObjectPool length does not fit in usize"))?;
            cluster.objs.push(Box::new(ObjectPool {
                length,
                entries: Vec::new(),
                extra_properties: HashMap::new(),
            }));
        }
    },
    |cluster, stream, _profile, _recipe| {
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

                obj.entries.push(ObjectPoolEntry {
                    entry_bits,
                    value,
                    extra_properties: HashMap::new(),
                });
            }
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Map, MapCluster);
IMPLEMENT_PROFILED_CLUSTER!(Map, MapCluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Set, SetCluster);
IMPLEMENT_PROFILED_CLUSTER!(Set, SetCluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Instance, InstanceCluster);
IMPLEMENT_CUSTOM_PROFILED_CLUSTER!(
    Instance,
    INSTANCE,
    InstanceCluster,
    |cluster, stream, _profile, _recipe| {
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
    |cluster, stream, _profile, _recipe| {
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
IMPLEMENT_CUSTOM_PROFILED_CLUSTER!(
    TypedData,
    DELTA_ENCODED_TYPED_DATA,
    DeltaEncodedTypedDataCluster,
    |cluster, stream, _profile, _recipe| {
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
    |cluster, stream, _profile, _recipe| {
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
IMPLEMENT_PROFILED_CLUSTER!(TypedData, TypedDataCluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Class, ClassCluster);
IMPLEMENT_CUSTOM_PROFILED_CLUSTER!(
    Class,
    CLASS,
    ClassCluster,
    |cluster, stream, _profile, _recipe| {
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
    |cluster, stream, profile, recipe| {
        const TOP_LEVEL_CID_OFFSET: i32 = 1 << 20;

        for obj in &mut cluster.objs {
            execute_object_recipe(
                obj.as_mut(),
                &recipe.operations,
                stream,
                cluster.cid,
                profile,
            )?;
            if obj.id < TOP_LEVEL_CID_OFFSET {
                obj.unboxed_fields_bitmap = Some(stream.read_unsigned()?);
            }
        }
    }
);

DECLARE_VARIABLE_LENGTH_CLUSTER!(TypeArguments, TypeArgumentsCluster);
// a canonical cluster in the root loading unit carries the canonical
// hash-set layout after its ordinary allocation records
// so this is just for the canonical TypeArguments cluster (if any)
IMPLEMENT_PROFILED_CLUSTER!(TypeArguments, TypeArgumentsCluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(ExceptionHandlers, ExceptionHandlersCluster);
IMPLEMENT_PROFILED_CLUSTER!(ExceptionHandlers, ExceptionHandlersCluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Context, ContextCluster);
IMPLEMENT_PROFILED_CLUSTER!(Context, ContextCluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(ContextScope, ContextScopeCluster);
IMPLEMENT_PROFILED_CLUSTER!(ContextScope, ContextScopeCluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Mint, MintCluster);
IMPLEMENT_PROFILED_CLUSTER!(Mint, MintCluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Float32x4, Float32x4Cluster);
IMPLEMENT_PROFILED_CLUSTER!(Float32x4, Float32x4Cluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Float64x2, Float64x2Cluster);
IMPLEMENT_PROFILED_CLUSTER!(Float64x2, Float64x2Cluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Record, RecordCluster);
IMPLEMENT_PROFILED_CLUSTER!(Record, RecordCluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(Array, ArrayCluster);
IMPLEMENT_PROFILED_CLUSTER!(Array, ArrayCluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(WeakArray, WeakArrayCluster);
IMPLEMENT_PROFILED_CLUSTER!(WeakArray, WeakArrayCluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(ImmutableArray, ImmutableArrayCluster);
IMPLEMENT_PROFILED_CLUSTER!(ImmutableArray, ImmutableArrayCluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(ConstMap, ConstMapCluster);
IMPLEMENT_PROFILED_CLUSTER!(ConstMap, ConstMapCluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(ConstSet, ConstSetCluster);
IMPLEMENT_PROFILED_CLUSTER!(ConstSet, ConstSetCluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(CodeSourceMap, CodeSourceMapCluster);
IMPLEMENT_PROFILED_CLUSTER!(CodeSourceMap, CodeSourceMapCluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(CompressedStackMaps, CompressedStackMapsCluster);
IMPLEMENT_PROFILED_CLUSTER!(CompressedStackMaps, CompressedStackMapsCluster, false);

DECLARE_VARIABLE_LENGTH_CLUSTER!(PcDescriptors, PcDescriptorsCluster);
// its saved twice
IMPLEMENT_PROFILED_CLUSTER!(PcDescriptors, PcDescriptorsCluster, false);

//DECLARE_VARIABLE_LENGTH_CLUSTER!(OneByteString, OneByteStringCluster); These only exist when NO COMPRESSED_POINTERS
//DECLARE_VARIABLE_LENGTH_CLUSTER!(TwoByteString, TwoByteStringCluster);
DECLARE_VARIABLE_LENGTH_CLUSTER!(_String, _StringCluster);
IMPLEMENT_CUSTOM_PROFILED_CLUSTER!(
    _String,
    STRING,
    _StringCluster,
    |cluster, stream, _profile, _recipe| {
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
        if cluster.is_canonical && _recipe.canonical_set_layout {
            read_canonical_set_layout(cluster.obj_count, stream)?;
        }
    },
    |cluster, stream, _profile, _recipe| {
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

    fn data_profile() -> DataProfile {
        crate::profile_reader::select_data_profile(
            &crate::constants::DART_3_11_1_SNAPSHOT_HASH.to_owned(),
            &vec![
                "product".into(),
                "arm64".into(),
                "compressed-pointers".into(),
            ],
        )
        .unwrap()
    }

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

    fn leb_signed(mut value: i64) -> Vec<u8> {
        let mut bytes = Vec::new();
        while !(-64..=63).contains(&value) {
            bytes.push((value & 0x7f) as u8);
            value >>= 7;
        }
        bytes.push((value + 0xc0) as u8);
        bytes
    }

    fn encoded_ref_id(mut value: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        loop {
            bytes.push((value & 0x7f) as u8);
            value >>= 7;
            if value == 0 {
                break;
            }
        }
        bytes.reverse();
        *bytes.last_mut().unwrap() |= 0x80;
        bytes
    }

    #[test]
    fn code_custom_handler_uses_profile_fields_for_normal_and_deferred_objects() {
        let profile = data_profile();
        let mut bytes = leb_unsigned(1);
        bytes.extend(leb_signed(-10));
        bytes.extend(leb_unsigned(1));
        bytes.extend(leb_signed(23));
        let alloc_end = bytes.len();
        bytes.extend(leb_unsigned((6 << 1) | 1));
        for id in 100..106 {
            bytes.extend(encoded_ref_id(id));
        }
        for id in 200..206 {
            bytes.extend(encoded_ref_id(id));
        }
        let fill_end = bytes.len();
        bytes.push(0x5a);

        let mut stream = Stream::new(&bytes);
        let mut cluster = CodeCluster::default();
        let mut next_ref = 7;
        cluster
            .read_alloc(&mut next_ref, &mut stream, &profile)
            .unwrap();

        assert_eq!(cluster.obj_count, 2);
        assert_eq!(cluster.non_deferred_obj_count, 1);
        assert_eq!(cluster.deferred_obj_count, 1);
        assert_eq!(cluster.first_ref_id, 7);
        assert_eq!(next_ref, 9);
        assert_eq!(cluster.objs[0].state_bits, -10);
        assert_eq!(cluster.objs[1].state_bits, 23);
        assert_eq!(stream.get_current_pos(), alloc_end);

        cluster.read_fill(&mut stream, &profile).unwrap();

        for (code, first_ref) in cluster.objs.iter().zip([100, 200]) {
            assert_eq!(code.owner, first_ref);
            assert_eq!(code.exception_handlers, first_ref + 1);
            assert_eq!(code.pc_descriptors, first_ref + 2);
            assert_eq!(code.catch_entry, first_ref + 3);
            assert_eq!(code.inlined_id_to_function, first_ref + 4);
            assert_eq!(code.code_source_map, first_ref + 5);
            assert_eq!(code.entry_point, u64::MAX);
            assert_eq!(code.monomorphic_entry_point, u64::MAX);
            assert_eq!(code.object_pool, 0);
            assert_eq!(code.compressed_stackmaps, 0);
        }
        assert!(cluster.objs[0].has_monomorphic_entrypoint);
        assert_eq!(cluster.objs[0].unchecked_entry_point, 6);
        assert_eq!(cluster.objs[0].monomorphic_unchecked_entry_point, 6);
        assert_eq!(cluster.objs[1].unchecked_entry_point, u64::MAX);
        assert_eq!(cluster.objs[1].monomorphic_unchecked_entry_point, u64::MAX);
        assert_eq!(stream.get_current_pos(), fill_end);
        assert_eq!(stream.read_byte().unwrap(), 0x5a);
    }

    #[test]
    fn class_custom_handler_reads_profile_fields_and_only_non_top_level_bitmaps() {
        let mut profile = data_profile();
        profile.parsing_operations.get_mut(CLASS).unwrap()[READ_FILL][FOR_EACH] = serde_json::json!([
            {"operation": "core", "target": "id", "codec": "signed"},
            {"operation": "core", "target": "name", "codec": "ref_id"}
        ]);

        let top_level_cid_offset = 1 << 20;
        let mut bytes = leb_unsigned(0);
        bytes.extend(leb_unsigned(2));
        bytes.extend(leb_signed(top_level_cid_offset - 1));
        bytes.extend(encoded_ref_id(123));
        bytes.extend(leb_unsigned(0x55aa));
        bytes.extend(leb_signed(top_level_cid_offset));
        bytes.extend(encoded_ref_id(456));
        let fill_end = bytes.len();
        bytes.push(0xa7);

        let mut stream = Stream::new(&bytes);
        let mut cluster = ClassCluster::default();
        let mut next_ref = 1;
        cluster
            .read_alloc(&mut next_ref, &mut stream, &profile)
            .unwrap();
        cluster.read_fill(&mut stream, &profile).unwrap();

        assert_eq!(cluster.obj_count, 2);
        assert_eq!(next_ref, 3);
        assert_eq!(cluster.objs[0].id, (top_level_cid_offset - 1) as i32);
        assert_eq!(cluster.objs[0].name, 123);
        assert_eq!(cluster.objs[0].unboxed_fields_bitmap, Some(0x55aa));
        assert_eq!(cluster.objs[1].id, top_level_cid_offset as i32);
        assert_eq!(cluster.objs[1].name, 456);
        assert_eq!(cluster.objs[1].unboxed_fields_bitmap, None);
        assert_eq!(stream.get_current_pos(), fill_end);
        assert_eq!(stream.read_byte().unwrap(), 0xa7);
    }

    #[test]
    fn function_fill_follows_profile_operation_order() {
        let mut profile = data_profile();
        profile.parsing_operations.get_mut(FUNCTION).unwrap()[READ_FILL][FOR_EACH] = serde_json::json!([
            {"operation": "core", "target": "data", "codec": "ref_id"},
            {"operation": "core", "target": "code_index", "codec": "unsigned"},
            {"operation": "core", "target": "owner", "codec": "ref_id"},
            {"operation": "core", "target": "kind_tag", "codec": "signed"},
            {"operation": "core", "target": "signature", "codec": "ref_id"},
            {"operation": "core", "target": "name", "codec": "ref_id"}
        ]);

        let mut bytes = leb_unsigned(1);
        bytes.extend(encoded_ref_id(101));
        bytes.extend(leb_unsigned(37));
        bytes.extend(encoded_ref_id(202));
        bytes.extend(leb_signed(-1));
        bytes.extend(encoded_ref_id(303));
        bytes.extend(encoded_ref_id(501));

        let mut stream = Stream::new(&bytes);
        let mut cluster = FunctionCluster::default();
        let mut next_ref = 17;
        cluster
            .read_alloc(&mut next_ref, &mut stream, &profile)
            .unwrap();
        cluster.read_fill(&mut stream, &profile).unwrap();

        let function = &cluster.objs[0];
        assert_eq!(function.data, 101);
        assert_eq!(function.code_index, 37);
        assert_eq!(function.owner, 202);
        assert_eq!(function.kind_tag, u32::MAX);
        assert_eq!(function.signature, 303);
        assert_eq!(function.name, 501);
        assert_eq!(function.entry_point, u64::MAX);
        assert_eq!(function.unchecked_entry_point, u64::MAX);
        assert_eq!(next_ref, 18);
        assert_eq!(stream.get_current_pos(), bytes.len());
    }

    #[test]
    fn core_reference_arrays_fill_typed_vectors_and_preserve_stream_position() {
        let profile = data_profile();
        let mut bytes = leb_unsigned(2);
        bytes.extend(leb_unsigned(3));
        bytes.extend(leb_unsigned(0));
        bytes.extend(leb_unsigned(3));
        bytes.extend(encoded_ref_id(42));
        for id in [1, 127, 128] {
            bytes.extend(encoded_ref_id(id));
        }
        bytes.extend(leb_unsigned(0));
        bytes.extend(encoded_ref_id(0));

        let mut stream = Stream::new(&bytes);
        let mut cluster = ArrayCluster::default();
        let mut next_ref = 10;
        cluster
            .read_alloc(&mut next_ref, &mut stream, &profile)
            .unwrap();
        cluster.read_fill(&mut stream, &profile).unwrap();

        assert_eq!(cluster.objs[0].elements, vec![1, 127, 128]);
        assert_eq!(cluster.objs[0].type_arguments, 42);
        assert!(cluster.objs[1].elements.is_empty());
        assert_eq!(cluster.objs[1].type_arguments, 0);
        assert_eq!(next_ref, 12);
        assert_eq!(stream.get_current_pos(), bytes.len());
    }

    #[test]
    fn exception_handler_arrays_use_the_child_vocabulary_and_signed_values() {
        let profile = data_profile();
        let mut bytes = leb_unsigned(1);
        bytes.extend(leb_unsigned(2));
        bytes.extend(leb_unsigned(4));
        bytes.extend(encoded_ref_id(300));
        bytes.extend(leb_signed(511));
        bytes.extend(leb_signed(-1));
        bytes.extend([0xff, 1, 0]);
        bytes.extend(leb_signed(1024));
        bytes.extend(leb_signed(0));
        bytes.extend([0, 0, 1]);

        let mut stream = Stream::new(&bytes);
        let mut cluster = ExceptionHandlersCluster::default();
        let mut next_ref = 1;
        cluster
            .read_alloc(&mut next_ref, &mut stream, &profile)
            .unwrap();
        cluster.read_fill(&mut stream, &profile).unwrap();

        let handlers = &cluster.objs[0];
        assert_eq!(handlers.handled_types_data, 300);
        assert_eq!(handlers.packed_fields, 4);
        assert_eq!(handlers.entries.len(), 2);
        assert_eq!(handlers.entries[0].handler_pc_offset, 511);
        assert_eq!(handlers.entries[0].outer_try_index, -1);
        assert_eq!(handlers.entries[0].needs_stacktrace, -1);
        assert_eq!(handlers.entries[0].has_catch_all, 1);
        assert_eq!(handlers.entries[1].handler_pc_offset, 1024);
        assert_eq!(handlers.entries[1].outer_try_index, 0);
        assert_eq!(handlers.entries[1].is_generated, 1);
        assert_eq!(stream.get_current_pos(), bytes.len());
    }

    #[test]
    fn canonical_layout_is_consumed_after_allocation_records() {
        let mut profile = data_profile();
        profile.parsing_operations.get_mut(TYPE).unwrap()[READ_ALLOC][FOR_EACH] = serde_json::json!([
            {"operation": "core", "target": "flags", "codec": "unsigned"}
        ]);

        let mut bytes = leb_unsigned(3);
        for flags in [10, 20, 30] {
            bytes.extend(leb_unsigned(flags));
        }
        for value in [8, 1, 0, 2] {
            bytes.extend(leb_unsigned(value));
        }
        let expected_end = bytes.len();
        bytes.push(0xee);

        let mut stream = Stream::new(&bytes);
        let mut cluster = TypeCluster::default();
        cluster.set_metadata(0, ClassKind::Type as u32, false, true);
        let mut next_ref = 1;
        cluster
            .read_alloc(&mut next_ref, &mut stream, &profile)
            .unwrap();

        assert_eq!(
            cluster
                .objs
                .iter()
                .map(|object| object.flags)
                .collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
        assert_eq!(next_ref, 4);
        assert_eq!(stream.get_current_pos(), expected_end);
        assert_eq!(stream.read_byte().unwrap(), 0xee);
    }

    #[test]
    fn context_lengths_drive_extra_arrays_and_discard_only_arrays() {
        let mut profile = data_profile();
        profile.parsing_operations.get_mut(FUNCTION).unwrap()[READ_FILL][FOR_EACH] = serde_json::json!([
            {"operation": "context", "slot": "amount", "codec": "unsigned"},
            {
                "operation": "extra", "target": "extra_refs",
                "codec": {"array": {
                    "length_from": "amount",
                    "for_each": [
                        {"operation": "core", "target": "value", "codec": "ref_id"},
                        {"operation": "discard", "codec": "byte"}
                    ]
                }}
            },
            {
                "operation": "discard",
                "codec": {"array": {
                    "length_from": "amount",
                    "for_each": [
                        {"operation": "discard", "codec": "signed"},
                        {"operation": "discard", "codec": "byte"}
                    ]
                }}
            },
            {"operation": "core", "target": "code_index", "codec": "unsigned"}
        ]);

        let mut bytes = leb_unsigned(1);
        bytes.extend(leb_unsigned(3));
        for id in [7, 128, 4096] {
            bytes.extend(encoded_ref_id(id));
            bytes.push(0x44);
        }
        for value in [-1, 63, -64] {
            bytes.extend(leb_signed(value));
            bytes.push(0x55);
        }
        bytes.extend(leb_unsigned(97));
        let expected_end = bytes.len();
        bytes.push(0x77);

        let mut stream = Stream::new(&bytes);
        let mut cluster = FunctionCluster::default();
        let mut next_ref = 1;
        cluster
            .read_alloc(&mut next_ref, &mut stream, &profile)
            .unwrap();
        cluster.read_fill(&mut stream, &profile).unwrap();

        let function = &cluster.objs[0];
        assert_eq!(function.code_index, 97);
        assert_eq!(function.extra_properties.len(), 1);
        assert_eq!(
            function.extra_properties.get("extra_refs"),
            Some(&PropType::Array(vec![
                PropType::RefId(7),
                PropType::RefId(128),
                PropType::RefId(4096)
            ]))
        );
        assert!(!function.extra_properties.contains_key("amount"));
        assert_eq!(stream.get_current_pos(), expected_end);
        assert_eq!(stream.read_byte().unwrap(), 0x77);
    }

    #[test]
    fn missing_profile_recipes_error_before_consuming_allocation_bytes() {
        let mut profile = data_profile();
        profile.parsing_operations.remove(FUNCTION);
        let bytes = leb_unsigned(1);
        let mut stream = Stream::new(&bytes);
        let mut cluster = FunctionCluster::default();
        let mut next_ref = 1;
        let error = cluster
            .read_alloc(&mut next_ref, &mut stream, &profile)
            .unwrap_err();

        assert!(format!("{error:#}").contains("function.read_alloc"));
        assert_eq!(stream.get_current_pos(), 0);
        assert_eq!(next_ref, 1);
    }

    #[test]
    fn unknown_profile_operation_and_core_target_error() {
        for operation in [
            serde_json::json!({"operation": "teleport", "codec": "byte"}),
            serde_json::json!({
                "operation": "core", "target": "unknown_field", "codec": "byte"
            }),
        ] {
            let mut profile = data_profile();
            profile.parsing_operations.get_mut(FUNCTION).unwrap()[READ_FILL][FOR_EACH] =
                serde_json::json!([operation]);
            let bytes = leb_unsigned(1);
            let mut stream = Stream::new(&bytes);
            let mut cluster = FunctionCluster::default();
            let mut next_ref = 1;
            cluster
                .read_alloc(&mut next_ref, &mut stream, &profile)
                .unwrap();
            let error = cluster.read_fill(&mut stream, &profile).unwrap_err();

            let message = format!("{error:#}");
            assert!(message.contains("decode function.read_fill.for_each"));
            assert!(message.contains("unknown variant"));
            assert_eq!(stream.get_current_pos(), bytes.len());
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
        let profile = data_profile();
        cluster
            .read_alloc(&mut next_ref, &mut stream, &profile)
            .unwrap();
        cluster.read_fill(&mut stream, &profile).unwrap();
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
    fn typed_data_fill_uses_profile_cid_mapping() {
        let raw_cid = ClassKind::TypedDataInt16Array as u32 + 1;
        let mut profile = data_profile();
        profile
            .cid_map
            .insert(raw_cid, ClassKind::TypedDataInt16Array);

        let mut bytes = leb_unsigned(1);
        bytes.extend_from_slice(&leb_unsigned(2));
        bytes.extend_from_slice(&leb_unsigned(2));
        bytes.extend_from_slice(&[1, 2, 3, 4]);

        let mut cluster = TypedDataCluster::default();
        cluster.set_metadata(0, raw_cid, false, false);
        let mut stream = Stream::new(&bytes);
        let mut next_ref = 1;
        cluster
            .read_alloc(&mut next_ref, &mut stream, &profile)
            .unwrap();
        cluster.read_fill(&mut stream, &profile).unwrap();

        assert_eq!(cluster.objs[0].data, [1, 2, 3, 4]);
    }

    #[test]
    fn external_typed_data_fill_uses_profile_cid_mapping() {
        let raw_cid = ClassKind::ExternalTypedDataInt16Array as u32 + 1;
        let mut profile = data_profile();
        profile
            .cid_map
            .insert(raw_cid, ClassKind::ExternalTypedDataInt16Array);

        let mut bytes = leb_unsigned(1);
        bytes.extend_from_slice(&leb_unsigned(2));
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&[1, 2, 3, 4]);

        let mut cluster = ExternalTypedDataCluster::default();
        cluster.set_metadata(0, raw_cid, false, false);
        let mut stream = Stream::new(&bytes);
        let mut next_ref = 1;
        cluster
            .read_alloc(&mut next_ref, &mut stream, &profile)
            .unwrap();
        cluster.read_fill(&mut stream, &profile).unwrap();

        assert_eq!(cluster.objs[0].data, [1, 2, 3, 4]);
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
        let profile = data_profile();
        cluster
            .read_alloc(&mut next_ref, &mut stream, &profile)
            .unwrap();
        assert!(cluster.read_fill(&mut stream, &profile).is_err());
    }
}
