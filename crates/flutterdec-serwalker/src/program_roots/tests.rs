use super::{parse_dispatch_table, DispatchTableEntry};
use crate::constants::{SIGNED_M, UNSIGNED_M};
use crate::stream::Stream;

fn write_unsigned(bytes: &mut Vec<u8>, mut value: u64) {
    while value > 0x7f {
        bytes.push((value & 0x7f) as u8);
        value >>= 7;
    }
    bytes.push(value as u8 + UNSIGNED_M);
}

fn write_signed(bytes: &mut Vec<u8>, mut value: i64) {
    while !(-64..=63).contains(&value) {
        bytes.push((value & 0x7f) as u8);
        value >>= 7;
    }
    bytes.push((value + i64::from(SIGNED_M)) as u8);
}

fn dispatch_bytes(length: u64, tokens: impl IntoIterator<Item = i64>) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_unsigned(&mut bytes, length);
    write_unsigned(&mut bytes, 100);
    for token in tokens {
        write_signed(&mut bytes, token);
    }
    bytes
}

#[test]
fn dispatch_table_can_reuse_all_64_recent_slots() {
    let fresh = (1..=64).map(|index| 64 + index);
    let reused = (0..64).map(|slot| !slot);
    let mut bytes = dispatch_bytes(128, fresh.chain(reused));
    assert_eq!(bytes.last(), Some(&0x80));
    let table_end = bytes.len();
    bytes.push(0x5a);

    let mut stream = Stream::new(&bytes);
    let table = parse_dispatch_table(&mut stream).unwrap();
    let expected: Vec<_> = (1..=64)
        .chain(1..=64)
        .map(DispatchTableEntry::CodeIndex)
        .collect();

    assert_eq!(
        table.entries.last(),
        Some(&DispatchTableEntry::CodeIndex(64))
    );
    assert_eq!(table.entries, expected);
    assert_eq!(table.first_code_ref, Some(100));
    assert_eq!(stream.get_current_pos(), table_end);
    assert_eq!(stream.read_byte().unwrap(), 0x5a);
}

#[test]
fn dispatch_table_recent_ring_wraps_after_64_fresh_entries() {
    let fresh = (1..=65).map(|index| 64 + index);
    let bytes = dispatch_bytes(67, fresh.chain([-64, -1]));
    let table = parse_dispatch_table(&mut Stream::new(&bytes)).unwrap();

    assert_eq!(
        &table.entries[65..],
        &[
            DispatchTableEntry::CodeIndex(64),
            DispatchTableEntry::CodeIndex(65)
        ]
    );
}

#[test]
fn dispatch_table_maximum_repeat_remains_63() {
    let bytes = dispatch_bytes(64, [64 + 7, 63]);
    let table = parse_dispatch_table(&mut Stream::new(&bytes)).unwrap();

    assert_eq!(table.entries, vec![DispatchTableEntry::CodeIndex(7); 64]);
}

#[test]
fn dispatch_table_rejects_an_uninitialized_recent_slot_63() {
    let bytes = dispatch_bytes(1, [-64]);
    let error = parse_dispatch_table(&mut Stream::new(&bytes)).unwrap_err();

    assert!(error
        .to_string()
        .contains("reference 63 appears before it is defined"));
}
