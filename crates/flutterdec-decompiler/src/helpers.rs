use super::LiftState;
use flutterdec_ir::FunctionIr;
use std::collections::BTreeSet;

include!("helpers/registers.rs");
include!("helpers/expr.rs");
include!("helpers/instruction_parse.rs");
include!("helpers/naming.rs");
include!("helpers/selector_table.rs");
include!("helpers/call_intent.rs");
include!("helpers/state_and_flow.rs");
