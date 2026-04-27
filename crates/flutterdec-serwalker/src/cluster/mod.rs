use crate::constants::{ClassId, ClassId::*, SIGNED_M, UNSIGNED_M};
use crate::raw_object::*;
use crate::DECLARE_FIXED_LENGTH_CLUSTER;
use crate::{constants, stream::Stream};

type Smi = i32;

pub trait Cluster {
    fn is_fixed_len(&self) -> bool;
    fn read_alloc(&mut self, last_ref_id: &mut u64, stream: &mut Stream) -> usize;
    fn read_fill(&mut self, last_ref_id: &mut u64, stream: &mut Stream) -> usize;
}

pub fn read_cluster_alloc() {
    let curr_ref_id: u64 = 0;
}

pub fn read_cluster_fill() {
    let curr_ref_id: u64 = 0;
}

pub fn read_and_decompress_smi(stream: &mut Stream) -> Smi {
<<<<<<< HEAD
    let raw_smi = stream.read_modified_leb128(SIGNED_M); // smis are always written as signed
=======
    let raw_smi = stream.read_modified_leb128(UNSIGNED_M); // smis are always written as signed
>>>>>>> b36733ae4d7105ce3e276c11bb3a44f57b3cccff

    (raw_smi as Smi) >> constants::SMI_SHIFT
}

pub fn decide_cluster(
    clusters: &mut [Box<dyn Cluster>; constants::MAX_CLUSTER_NUM],
    class_id: ClassId,
) -> Result<Box<dyn Cluster>, &str> {
    match class_id {
        IllegalCid => Err("Not a supported class (illegal class)..."),
        _ => Err("Not a supported class..."),
    }
}

// These are the objects that call ReadAllocFixedSize during deserialization,
// whose fill cluster size is uniquely determined by sizeof(Object) * num_of_objects
// and alloc cluster size is tags (MULEB128) + num_of_objects (MULEB128)

DECLARE_FIXED_LENGTH_CLUSTER!(OneByteString, {
    1
    // to-do
});
/*
DECLARE_FIXED_LENGTH_CLUSTER!(TwoByteString, 8);
DECLARE_FIXED_LENGTH_CLUSTER!(String, 8);
DECLARE_FIXED_LENGTH_CLUSTER!(Mint, 16);
DECLARE_FIXED_LENGTH_CLUSTER!(Double, 16);
DECLARE_FIXED_LENGTH_CLUSTER!(TypeParameter, 32);
DECLARE_FIXED_LENGTH_CLUSTER!(Type, 32);
DECLARE_FIXED_LENGTH_CLUSTER!(TypeArguments, 32);
*/
