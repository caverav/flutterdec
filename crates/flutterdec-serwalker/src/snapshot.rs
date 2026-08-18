use std::collections::HashMap;

use crate::cluster::{decide_cluster, resolve_entrypoints, Cluster};
use crate::constants::{
    self, DART_3_11_1_SNAPSHOT_HASH, MAGIC_BYTES, OBJECT_START_ALIGNMENT, SNAPSHOT_MAGIC_NUMBER_SZ,
};
use crate::instruction_table::{parse_instr_table_from_rodata, InstructionTable};
use crate::program_roots::structs::ProgramRoots;
use crate::program_roots::{parse_dispatch_table, parse_field_table, parse_object_store};
use crate::stream::Stream;
use crate::utils::{decode_tags, DecodedTags};

#[derive(Default)]
enum SnapshotKind
// Snapshot::Kind, snapshot.h:24. There is no kModule variant.
{
    Full,
    FullCore,
    FullJIT,
    FullAOT, // Full + AOT code, this is the one we care about, as this is how flutter builds projects
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
            4 => Ok(SnapshotKind::None),
            5 => Ok(SnapshotKind::Invalid),
            _ => Err("Invalid snapshot kind: header corrupt, or not a snapshot at all."),
        }
    }
}

#[derive(Default)]
pub struct DataSnapshot {
    pub clusters: HashMap<u32, Box<dyn Cluster>>,
    cluster_order: Vec<u32>, // used in the fill step to know which cluster's read_fill function to call
    roots: ProgramRoots,
    instruction_table: InstructionTable,

    magic_bytes: u32,
    clustered_size: u64,
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

        self.clustered_size = stream
            .read_raw_u64()?
            .checked_add(SNAPSHOT_MAGIC_NUMBER_SZ as u64)
            .ok_or_else(|| anyhow::anyhow!("snapshot length overflow"))?;
        self.kind =
            SnapshotKind::try_from(stream.read_raw_u64()?).map_err(|e| anyhow::anyhow!(e))?;

        if !matches!(&self.kind, SnapshotKind::FullAOT) {
            anyhow::bail!("Serwalker currently supports FullAOT snapshots only");
        }

        self.parse_version_and_features(stream)?;

        if self.version_hash != DART_3_11_1_SNAPSHOT_HASH {
            anyhow::bail!(
                "unsupported Dart snapshot hash {}; expected {} (Dart 3.11.1)",
                self.version_hash,
                DART_3_11_1_SNAPSHOT_HASH
            );
        }

        if !self
            .features
            .split_ascii_whitespace()
            .any(|feature| feature == "compressed-pointers")
        {
            anyhow::bail!("Serwalker currently requires a compressed-pointers snapshot");
        }

        self.num_base_objects = stream.read_unsigned()?;
        self.num_objects = stream.read_unsigned()?;
        self.num_clusters = stream.read_unsigned()?;

        self.instr_table_len = stream.read_unsigned()? as usize;
        self.instr_table_offset = stream.read_unsigned()? as usize;
        Ok(())
    }

    fn parse_clusters(&mut self, stream: &mut Stream) -> anyhow::Result<()> {
        let mut curr_ref_id: u64 = self.num_base_objects + 1; // all objects are numbered starting from num_base_objects + 1

        self.start_of_alloc_area = stream.get_current_pos();
        for _cluster_idx in 0..self.num_clusters {
            let tags: u32 = stream.read()? as u32;
            let decoded_tags: DecodedTags = decode_tags(tags)?;
            let cid = decoded_tags.get_cid();

            let mut cluster = decide_cluster(cid).map_err(|_| {
                anyhow::anyhow!("Couldn't find cluster implementation for class {:?}", cid)
            })?;

            cluster.set_metadata(
                tags,
                cid,
                decoded_tags.is_immutable(),
                decoded_tags.is_canonical(),
            );
            cluster.read_alloc(&mut curr_ref_id, stream)?;

            // Composite key exactly as suggested to PR reviewer
            let key = (cid as u32) << 2
                | ((decoded_tags.is_canonical() as u32) << 1)
                | (decoded_tags.is_immutable() as u32);
            self.clusters.insert(key, cluster);
            self.cluster_order.push(key);
        }
        self.end_of_alloc_area = stream.get_current_pos();

        // ASSERT_EQUAL(next_ref_index_ - kFirstReference, num_objects_)
        // app_snapshot.cc:9591. Cheapest possible desync detector.
        let allocated = curr_ref_id - 1;
        if allocated != self.num_objects {
            anyhow::bail!(
                "alloc pass allocated {allocated} refs, header declares {}",
                self.num_objects
            );
        }

        self.start_of_fill_area = stream.get_current_pos();
        for key in self.cluster_order.iter() {
            let cluster = self.clusters.get_mut(key).unwrap();
            (*cluster).read_fill(stream)?;
        }
        self.end_of_fill_area = stream.get_current_pos();
        Ok(())
    }

    fn parse_roots(&mut self, stream: &mut Stream) -> anyhow::Result<()> {
        let object_store = parse_object_store(stream)?;
        let field_table = parse_field_table(stream)?;
        let shared_field_table = parse_field_table(stream)?;
        let dispatch_table = parse_dispatch_table(stream)?;

        self.roots = ProgramRoots::new(
            object_store,
            field_table,
            shared_field_table,
            dispatch_table,
        );
        Ok(())
    }

    fn parse_instruction_table(&mut self, stream: &mut Stream) -> anyhow::Result<()> {
        self.instruction_table = parse_instr_table_from_rodata(stream)?;
        Ok(())
    }

    fn resolve_entrypoints(&mut self) -> anyhow::Result<()> {
        resolve_entrypoints(
            &mut self.clusters,
            &self.instruction_table,
            self.instr_table_len,
        )
    }
}

pub fn parse_snapshot(stream: &mut Stream) -> anyhow::Result<DataSnapshot> {
    let mut snapshot = DataSnapshot::default();

    println!("Now parsing the snapshot...");
    snapshot.parse_header(stream)?;
    snapshot.parse_clusters(stream)?;
    snapshot.parse_roots(stream)?;

    let clustered_end = usize::try_from(snapshot.clustered_size)
        .map_err(|_| anyhow::anyhow!("snapshot length does not fit in usize"))?;
    anyhow::ensure!(
        stream.get_current_pos() == clustered_end,
        "clustered snapshot ended at offset {}, but its header declares {clustered_end}",
        stream.get_current_pos()
    );

    stream.seek(clustered_end)?;
    stream.align_stream(OBJECT_START_ALIGNMENT)?;

    if snapshot.instr_table_offset == 0 {
        anyhow::ensure!(
            snapshot.instr_table_len == 0,
            "snapshot declares an instruction table but has no ROData offset"
        );
    } else {
        let instruction_table_start = stream
            .get_current_pos()
            .checked_add(snapshot.instr_table_offset)
            .ok_or_else(|| anyhow::anyhow!("instruction-table offset overflow"))?;
        stream.seek(instruction_table_start)?;
        snapshot.parse_instruction_table(stream)?;
    }

    snapshot.resolve_entrypoints()?;

    Ok(snapshot)
}
