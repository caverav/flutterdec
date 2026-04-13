
struct DataSnapshot
{
    clusters: Vec<&'static mut dyn Cluster>,

    magic_bytes: u32,
    size: u64,
    kind: u64,

    version: String,
    features: String,

    num_base_objects: u64,
    num_objects: u64,
    num_clusters: u64,

    instr_table_len: u64,
    instr_table_offset: u64,
}
trait Cluster
{
    fn is_fixed_len(&self) -> bool;
    fn get_size(&self) -> usize;
}

macro_rules! DECLARE_FIXED_LENGTH_CLUSTER
{
    ($name:ident, $instance_size:literal) => {
        struct $name
        {
            tags: u32,
            count: u64,
        }

        impl Cluster for $name 
        {
            fn get_size(&self) -> usize
            {
                $instance_size
            }

            fn is_fixed_len(&self) -> bool
            {
                true
            }
        }
    };
}

macro_rules! DECLARE_VARIABLE_LENGTH_CLUSTER
{
    ($name:ident, $get_length_impl:block) => {
        struct $name
        {
            tags: u32,
            count: u64
        }

        impl Cluster for $name 
        {
            fn get_size(&self) -> usize
                $get_length_impl

            fn is_fixed_len(&self) -> bool
            {
                false
            }
        }
    };
}

// These are the objects that call ReadAllocFixedSize during deserialization,
// whose fill cluster size is uniquely determined by sizeof(Object) * num_of_objects
// and alloc cluster size is tags (u32) + num_of_objects (ULEB128)

DECLARE_FIXED_LENGTH_CLUSTER!(OneByteStringCluster, 2);
DECLARE_FIXED_LENGTH_CLUSTER!(TwoByteStringCluster, 8);
DECLARE_FIXED_LENGTH_CLUSTER!(StringCluster, 8);
DECLARE_FIXED_LENGTH_CLUSTER!(MintCluster, 16);
DECLARE_FIXED_LENGTH_CLUSTER!(DoubleCluster, 16);
DECLARE_FIXED_LENGTH_CLUSTER!(TypeParameterCluster, 32);
DECLARE_FIXED_LENGTH_CLUSTER!(TypeCluster, 32);
DECLARE_FIXED_LENGTH_CLUSTER!(TypeArgumentsCluster, 32);
