use std::vec;

struct DataSnapshot
{
    clusters: Vec<&'static mut dyn Cluster>,
    
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
            count: u64
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

DECLARE_FIXED_LENGTH_CLUSTER!(OneByteStringCluster, 16);
DECLARE_FIXED_LENGTH_CLUSTER!(TwoByteStringCluster, 16);
