use std::collections::HashMap;

use crate::cluster::{decide_cluster, Cluster};
use crate::constants::{self, ClassId, MAGIC_BYTES, UNSIGNED_M};
use crate::stream::Stream;
use crate::utils::{decode_tags, DecodedTags};

#[derive(Default)]
enum SnapshotKind
// pulled straight out of the C++ def
{
    Full,
    FullCore,
    FullJIT,
    FullAOT, // Full + AOT code, this is the one we care about, as this is how flutter builds projects
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

#[derive(Default)]
pub struct DataSnapshot {
    clusters: HashMap<u32, Box<dyn Cluster>>,
    cluster_order: Vec<u32>, // used in the fill step to know which cluster's read_fill function to call

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

    end_of_alloc_area: usize,
    end_of_fill_area: usize,
}

impl DataSnapshot {
    fn parse_version_and_features(&mut self, stream: &mut Stream) -> anyhow::Result<()> {
        let mut version_and_features = stream.read_c_string()?;

        self.features = version_and_features.split_off(constants::VERSION_HASH_LENGTH); // returns (str[hash_len..])
        self.version_hash = version_and_features;
        Ok(())
    }

    fn parse_header(&mut self, stream: &mut Stream) -> anyhow::Result<()> {
        self.magic_bytes = stream.read_raw_u32()?;

        if self.magic_bytes != MAGIC_BYTES {
            anyhow::bail!("Not a snapshot...")
        }

        self.size = stream.read_raw_u64()?;
        self.kind = SnapshotKind::try_from(stream.read_raw_u64()?).map_err(|e| anyhow::anyhow!(e))?;

        self.parse_version_and_features(stream)?;

        self.num_base_objects = stream.read_unsigned()?;
        self.num_objects = stream.read_unsigned()?;
        self.num_clusters = stream.read_unsigned()?;

        self.instr_table_len = stream.read_unsigned()? as usize;
        self.instr_table_offset = stream.read_unsigned()? as usize;
        Ok(())
    }

    fn parse_clusters(&mut self, stream: &mut Stream) -> anyhow::Result<()> {
        let mut curr_ref_id: u64 = 0; // all objects are numbered starting from 0

        self.start_of_alloc_area = stream.get_current_pos();
        for _cluster_idx in 0..self.num_clusters {
            let tags: u32 = stream.read_unsigned()? as u32;
            let decoded_tags: DecodedTags = decode_tags(tags)?;
            let cid = decoded_tags.get_cid();

            let mut cluster = decide_cluster(cid).map_err(|_| {
                anyhow::anyhow!("Couldn't find cluster implementation for class {:?}", cid)
            })?;

            cluster.read_alloc(&mut curr_ref_id, stream)?;
            
            // Composite key exactly as suggested by PR reviewer
            let key = (cid as u32) << 2 | ((decoded_tags.is_canonical() as u32) << 1) | (decoded_tags.is_deeply_immutable() as u32);
            self.clusters.insert(key, cluster);
            self.cluster_order.push(key);
        }
        self.end_of_alloc_area = stream.get_current_pos();

        self.start_of_fill_area = stream.get_current_pos();
        for key in self.cluster_order.iter() {
            let cluster = self.clusters.get_mut(key).unwrap();
            (*cluster).read_fill(stream)?;
        }
        self.end_of_fill_area = stream.get_current_pos();
        Ok(())
    }

    fn parse_roots(&mut self, _stream: &mut Stream) -> anyhow::Result<()> {
        Ok(())
    }
}

pub fn parse_snapshot(stream: &mut Stream) -> anyhow::Result<DataSnapshot> {
    let mut snapshot = DataSnapshot::default();

    println!("Now parsing the snapshot...");
    snapshot.parse_header(stream)?;
    snapshot.parse_clusters(stream)?;
    snapshot.parse_roots(stream)?;

    Ok(snapshot)
}

