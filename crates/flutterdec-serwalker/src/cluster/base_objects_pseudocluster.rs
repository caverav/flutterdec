// this will be an AddBaseObjects implementation so we can resolve refids <= num_base_objects
// for refids < vm snapshot base objects, given those objects are NOT written to the snapshot but rather
// created by the Dart runtime at initialization time, before deserialization takes place
