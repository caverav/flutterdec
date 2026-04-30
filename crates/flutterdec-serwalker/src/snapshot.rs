use crate::cluster::Cluster;
use crate::constants::{self, MAGIC_BYTES, UNSIGNED_M};
use crate::stream::Stream;

#[derive(Default)]
enum SnapshotKind
// pulled straight out of the C++ def
{
    Full,
    FullCore,
    FullJIT,
    FullAOT, // Full + AOT code, this is the one we care about, as th
    Module,
    #[default]
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
    clusters: [Option<Box<dyn Cluster>>; constants::MAX_CLUSTER_NUM], // thus each cluster must have its own UNIQUE class id

    magic_bytes: u32,
    size: u64,
    kind: SnapshotKind,

    version_hash: String,
    features: String,

    num_base_objects: u64,
    num_objects: u64,
    num_clusters: u64,

    instr_table_len: usize,
    instr_table_offset: usize,

    start_of_alloc_area: usize,
    start_of_fill_area: usize,
}

impl Default for DataSnapshot {
    fn default() -> Self {
        const INIT_CLUSTER: Option<Box<dyn Cluster>> = None;
        Self {
            clusters: [INIT_CLUSTER; constants::MAX_CLUSTER_NUM],
            magic_bytes: 0,
            size: 0,
            kind: SnapshotKind::default(),
            version_hash: String::new(),
            features: String::new(),
            num_base_objects: 0,
            num_objects: 0,
            num_clusters: 0,
            instr_table_len: 0,
            instr_table_offset: 0,
            start_of_alloc_area: 0,
            start_of_fill_area: 0,
        }
    }
}

impl DataSnapshot {
    
    fn parse_version_and_features(&mut self, stream: &mut Stream) {
        let mut version_and_features = stream.read_c_string();

        self.features = version_and_features.split_off(constants::VERSION_HASH_LENGTH); // returns (str[hash_len..])
        self.version_hash = version_and_features;
    }

    fn parse_header(&mut self, stream: &mut Stream) {
        self.magic_bytes = stream.read_u32();

        if self.magic_bytes != MAGIC_BYTES {
            panic!("Not a snapshot...")
        }

        self.size = stream.read_u64();
        self.kind = SnapshotKind::try_from(stream.read_u64()).expect("Not a valid snapshot!");

        self.parse_version_and_features(stream);

        self.num_base_objects = stream.read_modified_leb128(UNSIGNED_M);
        self.num_objects = stream.read_modified_leb128(UNSIGNED_M);
        self.num_clusters = stream.read_modified_leb128(UNSIGNED_M);

        self.instr_table_len = stream.read_modified_leb128(UNSIGNED_M) as usize;
        self.instr_table_offset = stream.read_modified_leb128(UNSIGNED_M) as usize;

        self.start_of_alloc_area = stream.get_current_pos();
    }

    fn read_clusters(&mut self, stream: &mut Stream) {
        let curr_ref_id: u64 = 0; // all objects are numbered starting from 0
    }

    pub fn parse_snapshot(&mut self, stream: &mut Stream)
    {
        println!("Now parsing the snapshot...");
        self.parse_header(stream);
    }

}
