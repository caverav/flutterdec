use std::collections::HashMap;

use crate::cluster::{
    decide_cluster, resolve_entrypoints, resolve_instructions_len_for_code_objects, ClassCluster,
    Cluster, CodeCluster,
};
use crate::constants::{
    self, ClassKind, MAGIC_BYTES, OBJECT_START_ALIGNMENT, SNAPSHOT_MAGIC_NUMBER_SZ,
};
use crate::instruction_table::{parse_instr_table_from_rodata, InstructionTable};
use crate::profile_reader::{
    profile::{ClassTagKind, DataProfile},
    select_data_profile,
};
use crate::program_roots::structs::ProgramRoots;
use crate::program_roots::{parse_dispatch_table, parse_field_table, parse_object_store};
use crate::stream::Stream;
use crate::utils::{decode_tags_for_kind, DecodedTags};

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
pub struct InstructionsSnapshot {
    pub image_size: u64,
    pub image_va: u64,
}

#[derive(Default)]
pub struct DataSnapshot {
    pub clusters: HashMap<u32, Box<dyn Cluster>>,
    cluster_order: Vec<u32>, // used in the fill step to know which cluster's read_fill function to call
    pub(crate) roots: ProgramRoots,
    pub(crate) instruction_table: InstructionTable,
    pub(crate) class_table: HashMap<i32, usize>, // maps a CID to an index into ClassCluster::objs

    magic_bytes: u32,
    clustered_size: u64,
    kind: SnapshotKind,

    data_profile: DataProfile,
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
    pub(crate) fn cid_for_class_kind(&self, class_kind: ClassKind) -> anyhow::Result<u32> {
        self.data_profile
            .class_kind_to_cid
            .get(&class_kind)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("profile has no CID for {class_kind:?}"))
    }

    pub(crate) fn get_cluster<T: Cluster + 'static>(&self, kind: ClassKind) -> anyhow::Result<&T> {
        let cid = self.cid_for_class_kind(kind)?;
        let key = cid << 2;
        let cluster = self.clusters.get(&key).ok_or_else(|| {
            anyhow::anyhow!("flags-0 cluster for {kind:?} (CID {cid}) is missing")
        })?;

        cluster.as_any().downcast_ref::<T>().ok_or_else(|| {
            anyhow::anyhow!(
                "flags-0 cluster for {kind:?} (CID {cid}) is not {}",
                std::any::type_name::<T>()
            )
        })
    }

    pub(crate) fn get_cluster_mut<T: Cluster + 'static>(
        &mut self,
        kind: ClassKind,
    ) -> anyhow::Result<&mut T> {
        let cid = self.cid_for_class_kind(kind)?;
        let key = cid << 2;
        let cluster = self.clusters.get_mut(&key).ok_or_else(|| {
            anyhow::anyhow!("flags-0 cluster for {kind:?} (CID {cid}) is missing")
        })?;

        cluster.as_any_mut().downcast_mut::<T>().ok_or_else(|| {
            anyhow::anyhow!(
                "flags-0 cluster for {kind:?} (CID {cid}) is not {}",
                std::any::type_name::<T>()
            )
        })
    }

    pub(crate) fn cluster_by_ref_id(
        &self,
        reference_id: u32,
    ) -> anyhow::Result<Option<&dyn Cluster>> {
        let mut left = 0;
        let mut right = self.cluster_order.len();

        // use the fact that ref ids are sequential identifiers to setup a fast binary search
        while left < right {
            let middle = left + (right - left) / 2;
            let key = self.cluster_order[middle];
            let cluster = self.clusters.get(&key).ok_or_else(|| {
                anyhow::anyhow!("cluster_order contains missing cluster key {key}")
            })?;

            if cluster.first_ref_id() <= reference_id {
                left = middle + 1;
            } else {
                right = middle;
            }
        }

        let Some(candidate_index) = left.checked_sub(1) else {
            return Ok(None);
        };
        let key = self.cluster_order[candidate_index];
        let cluster = self
            .clusters
            .get(&key)
            .ok_or_else(|| anyhow::anyhow!("cluster_order contains missing cluster key {key}"))?;

        if cluster.object_by_ref_id(reference_id).is_none() {
            return Ok(None);
        }

        Ok(Some(cluster.as_ref()))
    }

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
            anyhow::bail!("Serwalker currently supports FullAOT snapshots only.");
        }

        self.parse_version_and_features(stream)?;

        let mut features = Vec::new();
        for feature in self.features.split_ascii_whitespace() {
            features.push(String::from(feature));
        }

        let data_profile = select_data_profile(&self.version_hash, &features)?;
        self.data_profile = data_profile;
        /*
        if !self
            .features
            .split_ascii_whitespace()
            .any(|feature| feature == "compressed-pointers")
        {
            anyhow::bail!("Serwalker currently requires a compressed-pointers snapshot");
        }
        */

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
            let encoded_tags = stream.read()?;
            let tags = match self.data_profile.class_tag_kind {
                ClassTagKind::PackedCidAndCanonical => u32::try_from(encoded_tags),
                ClassTagKind::ObjectHeader | ClassTagKind::ObjectHeaderDeeplyImmutable => {
                    u32::try_from(encoded_tags)
                        .or_else(|_| i32::try_from(encoded_tags as i64).map(|tags| tags as u32))
                }
            }
            .map_err(|_| anyhow::anyhow!("Cluster tags {encoded_tags:#x} exceed 32 bits"))?;
            let decoded_tags: DecodedTags =
                decode_tags_for_kind(tags, self.data_profile.class_tag_kind);
            let read_cid = decoded_tags.get_cid();

            let class_kind = if read_cid < self.data_profile.num_predefined_cids {
                *self.data_profile.cid_map.get(&read_cid).ok_or_else(|| {
                    anyhow::anyhow!("A suitable mapping wasn't found for CID: {read_cid}")
                })?
            } else {
                ClassKind::Instance
            };

            let mut cluster = decide_cluster(class_kind).map_err(|reason| {
                anyhow::anyhow!("Couldn't find cluster implementation for CID {read_cid}: {reason}")
            })?;

            cluster.set_metadata(
                tags,
                read_cid,
                decoded_tags.is_immutable(),
                decoded_tags.is_canonical(),
            );
            cluster.read_alloc(&mut curr_ref_id, stream, &self.data_profile)?;

            // Composite key exactly as suggested to PR reviewer
            let key = read_cid << 2
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
            (*cluster).read_fill(stream, &self.data_profile)?;
        }
        self.end_of_fill_area = stream.get_current_pos();
        self.build_class_table()?; // we need this to resolve class names associated with Type objects
        Ok(())
    }

    fn build_class_table(&mut self) -> anyhow::Result<()> {
        let class_table = {
            let class_cluster = self.get_cluster::<ClassCluster>(ClassKind::Class)?;

            let mut class_table = HashMap::with_capacity(class_cluster.objs.len());
            for (index, class) in class_cluster.objs.iter().enumerate() {
                if class.id == ClassKind::Illegal as i32 {
                    anyhow::bail!("class at cluster index {index} has an illegal CID");
                    // should never happen
                }

                if let Some(previous_index) = class_table.insert(class.id, index) {
                    anyhow::bail!(
                        "duplicate class CID {} at cluster indexes {previous_index} and {index}",
                        class.id
                    );
                }
            }

            class_table
        };

        self.class_table = class_table;
        Ok(())
    }

    fn parse_roots(&mut self, stream: &mut Stream) -> anyhow::Result<()> {
        let object_store = parse_object_store(stream, &self.data_profile)?;
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

pub fn parse_instr_snapshot(
    stream: &mut Stream,
    snapshot: &mut DataSnapshot,
) -> anyhow::Result<InstructionsSnapshot> {
    let mut instructions_snapshot = InstructionsSnapshot::default();
    instructions_snapshot.image_size = stream.read_raw_u64()?;

    let code_cluster = snapshot.get_cluster_mut::<CodeCluster>(ClassKind::Code)?;
    resolve_instructions_len_for_code_objects(
        code_cluster,
        instructions_snapshot.image_size as usize,
    )?;

    Ok(instructions_snapshot)
}

pub fn parse_data_snapshot(stream: &mut Stream) -> anyhow::Result<DataSnapshot> {
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

    snapshot.resolve_entrypoints()?; // this is the equivalent of the CodeDeserializationCluster's PostLoad operation
 
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{DART_3_11_1_SNAPSHOT_HASH, SIGNED_M, UNSIGNED_M};

    fn leb(mut value: u64, marker: u8) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let low = (value & 0x7f) as u8;
            let rest = value >> 7;
            if rest == 0 && low <= (0xff - marker) {
                out.push(low + marker);
                return out;
            }
            out.push(low);
            value = rest;
        }
    }

    /// Snapshot header, snapshot.h:36. magic(u32) + length(i64) + kind(i64) as
    /// raw little endian, then 32 version chars and a NUL terminated feature
    /// string, then five LEB128 counts.
    struct Header {
        kind: u64,
        version: String,
        features: String,
        num_base_objects: u64,
        num_objects: u64,
        num_clusters: u64,
    }

    impl Default for Header {
        fn default() -> Self {
            Self {
                kind: 3, // kFullAOT
                version: DART_3_11_1_SNAPSHOT_HASH.into(),
                features: "product no-code_comments arm64 compressed-pointers".into(),
                num_base_objects: 42,
                num_objects: 1000,
                num_clusters: 7,
            }
        }
    }

    impl Header {
        fn encode(&self, magic: u32) -> Vec<u8> {
            let mut b = Vec::new();
            b.extend_from_slice(&magic.to_le_bytes());
            b.extend_from_slice(&0u64.to_le_bytes());
            b.extend_from_slice(&self.kind.to_le_bytes());
            b.extend_from_slice(self.version.as_bytes());
            b.extend_from_slice(self.features.as_bytes());
            b.push(0);
            for v in [
                self.num_base_objects,
                self.num_objects,
                self.num_clusters,
                0, // instructions table length
                0, // instructions table offset
            ] {
                b.extend_from_slice(&leb(v, UNSIGNED_M));
            }
            b
        }
    }

    fn parse(bytes: &[u8]) -> anyhow::Result<DataSnapshot> {
        let mut snap = DataSnapshot::default();
        let mut stream = Stream::new(bytes);
        snap.parse_header(&mut stream)?;
        Ok(snap)
    }

    fn snapshot_with_class_cluster(cid: u32) -> DataSnapshot {
        let mut snapshot = DataSnapshot::default();
        snapshot
            .data_profile
            .class_kind_to_cid
            .insert(ClassKind::Class, cid);
        let mut cluster = ClassCluster::default();
        cluster.set_metadata(0, cid, false, false);
        cluster.obj_count = 3;
        snapshot.clusters.insert(cid << 2, Box::new(cluster));
        snapshot
    }

    #[test]
    fn typed_cluster_lookup_uses_profile_cid_and_allows_mutation() {
        let mut snapshot = snapshot_with_class_cluster(700);
        let cluster = snapshot
            .get_cluster::<ClassCluster>(ClassKind::Class)
            .unwrap();
        assert_eq!(cluster.cid(), 700);
        assert_eq!(cluster.obj_count, 3);

        snapshot
            .get_cluster_mut::<ClassCluster>(ClassKind::Class)
            .unwrap()
            .obj_count = 7;

        assert_eq!(
            snapshot
                .get_cluster::<ClassCluster>(ClassKind::Class)
                .unwrap()
                .obj_count,
            7
        );
    }

    #[test]
    fn typed_cluster_lookup_reports_missing_profile_mapping_and_cluster() {
        let mut snapshot = DataSnapshot::default();
        let error = snapshot
            .get_cluster::<ClassCluster>(ClassKind::Class)
            .err()
            .unwrap();
        assert!(error.to_string().contains("profile has no CID"));
        assert!(snapshot
            .get_cluster_mut::<ClassCluster>(ClassKind::Class)
            .is_err());

        snapshot
            .data_profile
            .class_kind_to_cid
            .insert(ClassKind::Class, 700);
        let error = snapshot
            .get_cluster::<ClassCluster>(ClassKind::Class)
            .err()
            .unwrap();
        assert!(error.to_string().contains("CID 700"));
        assert!(error.to_string().contains("is missing"));
        assert!(snapshot
            .get_cluster_mut::<ClassCluster>(ClassKind::Class)
            .is_err());
    }

    #[test]
    fn typed_cluster_lookup_rejects_wrong_rust_type() {
        let mut snapshot = snapshot_with_class_cluster(700);
        let error = snapshot
            .get_cluster::<CodeCluster>(ClassKind::Class)
            .err()
            .unwrap();
        assert!(error.to_string().contains("CID 700"));
        assert!(error.to_string().contains("CodeCluster"));
        assert!(snapshot
            .get_cluster_mut::<CodeCluster>(ClassKind::Class)
            .is_err());
        assert_eq!(
            snapshot
                .get_cluster::<ClassCluster>(ClassKind::Class)
                .unwrap()
                .obj_count,
            3
        );
    }

    #[test]
    fn typed_cluster_lookup_does_not_fall_back_to_flagged_clusters() {
        let mut snapshot = snapshot_with_class_cluster(700);
        snapshot.clusters.remove(&(700 << 2));
        for flags in 1..4 {
            let mut cluster = ClassCluster::default();
            cluster.set_metadata(0, 700, flags & 1 != 0, flags & 2 != 0);
            snapshot
                .clusters
                .insert((700 << 2) | flags, Box::new(cluster));
        }

        assert!(snapshot
            .get_cluster::<ClassCluster>(ClassKind::Class)
            .is_err());
        assert!(snapshot
            .get_cluster_mut::<ClassCluster>(ClassKind::Class)
            .is_err());
    }

    #[test]
    fn parses_a_well_formed_header() {
        let h = Header::default();
        let snap = parse(&h.encode(MAGIC_BYTES)).unwrap();
        assert_eq!(snap.magic_bytes, MAGIC_BYTES);
        assert_eq!(snap.num_base_objects, 42);
        assert_eq!(snap.num_objects, 1000);
        assert_eq!(snap.num_clusters, 7);
    }

    /// The version hash is 32 raw chars with no terminator and the features
    /// string runs to the NUL, so the split is positional. Getting the length
    /// wrong silently corrupts both fields rather than failing.
    #[test]
    fn splits_version_from_features_at_thirty_two_chars() {
        let h = Header::default();
        let snap = parse(&h.encode(MAGIC_BYTES)).unwrap();
        assert_eq!(snap.version_hash, h.version);
        assert_eq!(
            snap.version_hash.len(),
            crate::constants::VERSION_HASH_LENGTH
        );
        assert_eq!(snap.features, h.features);
    }

    #[test]
    fn rejects_a_bad_magic() {
        let h = Header::default();
        assert!(parse(&h.encode(0xdeadbeef)).is_err());
    }

    #[test]
    fn rejects_an_out_of_range_snapshot_kind() {
        let h = Header {
            kind: 99,
            ..Default::default()
        };
        assert!(parse(&h.encode(MAGIC_BYTES)).is_err());
    }

    /// kFull=0, kFullCore=1, kFullJIT=2, kFullAOT=3, kNone=4, kInvalid=5.
    /// There is no kModule, so 5 must be the last valid value.
    #[test]
    fn snapshot_kind_numbering_matches_dart() {
        for k in 0..=5u64 {
            assert!(
                SnapshotKind::try_from(k).is_ok(),
                "kind {k} should be valid"
            );
        }
        assert!(
            SnapshotKind::try_from(6).is_err(),
            "there is no seventh kind"
        );
        assert!(matches!(
            SnapshotKind::try_from(3),
            Ok(SnapshotKind::FullAOT)
        ));
        assert!(matches!(SnapshotKind::try_from(4), Ok(SnapshotKind::None)));
    }

    /// The counts are ReadUnsigned (0x80), not Read (0xC0). Encoding them with
    /// the other marker must not silently produce plausible numbers.
    #[test]
    fn header_counts_use_the_unsigned_marker() {
        let h = Header {
            num_objects: 1000,
            ..Default::default()
        };
        let good = parse(&h.encode(MAGIC_BYTES)).unwrap();
        assert_eq!(good.num_objects, 1000);

        // Re-encode just the counts with the signed marker.
        let mut b = h.encode(MAGIC_BYTES);
        let prefix = 20 + h.version.len() + h.features.len() + 1;
        b.truncate(prefix);
        for v in [h.num_base_objects, h.num_objects, h.num_clusters, 0, 0] {
            b.extend_from_slice(&leb(v, SIGNED_M));
        }
        let wrong = parse(&b).unwrap();
        assert_ne!(
            wrong.num_objects, 1000,
            "wrong marker must not decode cleanly"
        );
    }

    #[test]
    fn truncated_headers_error_instead_of_panicking() {
        let full = Header::default().encode(MAGIC_BYTES);
        for cut in [0, 4, 12, 20, 30, full.len() - 1] {
            assert!(
                parse(&full[..cut]).is_err(),
                "truncation at {cut} should error"
            );
        }
    }
}
