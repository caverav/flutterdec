use crate::cluster::Cluster;
use crate::constants::{self, MAGIC_BYTES};
use crate::stream::Stream;

enum SnapshotKind
// pulled straight out of the C++ def
{
    Full,
    FullCore,
    FullJIT,
    FullAOT, // Full + AOT code, this is the one we care about, as th
    Module,
    None,
    Invalid,
}

impl TryFrom<u64> for SnapshotKind {
    type Error = &'static str;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(SnapshotKind::Full),
            1 => Ok(SnapshotKind::FullCore),
            2 => Ok(SnapshotKind::FullJIT),
            3 => Ok(SnapshotKind::FullAOT),
            4 => Ok(SnapshotKind::Module),
            5 => Ok(SnapshotKind::None),
            6 => Ok(SnapshotKind::Invalid),
            _ => Err("Invalid snapshot kind... Either headers are corrupt, or this is not a snapshot at all."), // Handle invalid snapshot kidjns
        }
    }
}
struct DataSnapshot {
    // this array will contain mutable references to all clusters, and it will be indexed using the class id
    clusters: [Box<dyn Cluster>; constants::MAX_CLUSTER_NUM], // thus each cluster must have its own UNIQUE class id

    magic_bytes: u32,
    size: u64,
    kind: SnapshotKind,

    version_hash: String,
    features: String,

    num_base_objects: u64,
    num_objects: u64,
    num_clusters: u64,

    instr_table_len: u64,
    instr_table_offset: usize,

    start_of_alloc_area: usize,
    start_of_fill_area: usize,
}

impl DataSnapshot {
    fn parse_header(&mut self, stream: &mut Stream) {
        self.magic_bytes = stream.read_u32();

        if self.magic_bytes != MAGIC_BYTES {
            panic!("Not a snapshot...")
        }

        self.size = stream.read_u64();
        self.kind = SnapshotKind::try_from(stream.read_u64()).expect("Not a valid snapshot!");

        self.parse_version_and_features(stream);
    }

    fn parse_version_and_features(&mut self, stream: &mut Stream) {
        let mut version_and_features = stream.read_c_string();

        self.features = version_and_features.split_off(constants::VERSION_HASH_LENGTH); // returns (str[hash_len..])
        self.version_hash = version_and_features;
    }

    fn read_clusters(&mut self, stream: &Stream) {
        let curr_ref_id: u64 = 0; // all objects are numbered starting from 0
    }
}
