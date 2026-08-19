//! Byte determinism of the whole generated artifact set, across separate
//! complete pipeline processes.
//!
//! One synthetic ARM64 `libapp.so` plus one synthetic adapter model go in;
//! pseudocode, emitted IR, `quality.json` and `report.json` come out. Twenty
//! processes each run the complete `run_decompile` pipeline - loader, adapter,
//! disassembler, IR build, CFG and region analysis, emission, artifact writing -
//! into their own output directory, and the parent compares what they wrote.
//!
//! A process boundary is the only way to test this. Within one process the
//! hashers are seeded once, so twenty iterations of a loop re-use one iteration
//! order and would agree however order-dependent the pipeline was. Each child
//! also prints a value decided by `HashSet` iteration order and nothing else; if
//! all twenty agreed on that, the artifacts would agree for a reason that has
//! nothing to do with the pipeline being ordered, and the comparison would pass
//! vacuously forever.
//!
//! `quality.json`, the pseudocode and the emitted IR are compared byte for byte.
//! `report.json` carries absolute output paths, which necessarily differ between
//! processes writing to different directories. Those are the volatile scalars
//! frozen by the oracle protocol's section 6 allowlist, named here before any
//! comparison and never widened afterwards. The comparison locates their exact
//! raw byte ranges, checks each is the scalar type it is allowed to be, and then
//! compares every untouched raw slice around them in order. The document is
//! never parsed into a value and re-serialized: a normalizing round trip would
//! hide exactly the key-order and float-formatting drift this test exists to
//! catch.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use flutterdec_core::{
    run_decompile, AdapterBackend, DecompileAnalysisProfile, DecompileEngineOptions,
    DecompileOptions, FunctionScope,
};

/// Set on a child to make it run one pipeline instead of spawning twenty.
const CHILD_REQUEST: &str = "FLUTTERDEC_PIPELINE_DETERMINISM_DIR";
/// The child is filtered to this test by name, so the name is part of the
/// contract: a rename that misses it leaves the child running no test at all,
/// which the empty-canary assertion below refuses rather than passes.
const CHILD_TEST: &str = "the_whole_artifact_set_is_byte_identical_in_twenty_processes";
const CANARY_PREFIX: &str = "canary|";
const OUTCOME_PREFIX: &str = "outcome|";
const PROCESSES: usize = 20;

/// The snapshot hash the synthetic bundle advertises. Any 32 lowercase hex
/// characters; the adapter is installed under this name.
const SNAPSHOT_HASH: &str = "d0d0dec0ffee1234567890abcdef0123";

// ---------------------------------------------------------------------------
// AArch64 encoding
// ---------------------------------------------------------------------------

/// A two-pass label-resolving assembler for the handful of AArch64 forms the
/// fixtures need. Branch displacements are PC-relative, so the encoded program
/// does not depend on where the ELF finally places it.
mod asm {
    #[derive(Clone)]
    pub enum Ins {
        /// A fully encoded instruction.
        Word(u32),
        /// `b <label>`
        B(String),
        /// `b.<cond> <label>`
        Bc(u32, String),
        /// `cbz x<rt>, <label>`
        Cbz(u32, String),
        /// `bl <label>`
        Bl(String),
        /// Not an instruction: names the next address.
        Label(String),
    }

    pub const COND_EQ: u32 = 0;
    pub const COND_NE: u32 = 1;
    pub const COND_LT: u32 = 11;

    pub fn word(w: u32) -> Ins {
        Ins::Word(w)
    }
    pub fn label(name: &str) -> Ins {
        Ins::Label(name.to_string())
    }
    pub fn b(name: &str) -> Ins {
        Ins::B(name.to_string())
    }
    pub fn bc(cond: u32, name: &str) -> Ins {
        Ins::Bc(cond, name.to_string())
    }
    pub fn cbz(rt: u32, name: &str) -> Ins {
        Ins::Cbz(rt, name.to_string())
    }
    pub fn bl(name: &str) -> Ins {
        Ins::Bl(name.to_string())
    }

    /// `stp x29, x30, [sp, #-16]!` - the frame prologue.
    pub fn prologue() -> Ins {
        word(0xA9BF_7BFD)
    }
    /// `ldp x29, x30, [sp], #16`
    pub fn epilogue() -> Ins {
        word(0xA8C1_7BFD)
    }
    pub fn ret() -> Ins {
        word(0xD65F_03C0)
    }
    /// `movz x<rd>, #imm16`
    pub fn movz(rd: u32, imm16: u32) -> Ins {
        word(0xD280_0000 | (imm16 << 5) | rd)
    }
    /// `mov x<rd>, x<rm>` (`orr xd, xzr, xm`)
    pub fn mov(rd: u32, rm: u32) -> Ins {
        word(0xAA00_03E0 | (rm << 16) | rd)
    }
    /// `mov w<rd>, w<rm>` (`orr wd, wzr, wm`)
    pub fn mov_w(rd: u32, rm: u32) -> Ins {
        word(0x2A00_03E0 | (rm << 16) | rd)
    }
    /// `add x<rd>, x<rn>, #imm12`
    pub fn add_imm(rd: u32, rn: u32, imm12: u32) -> Ins {
        word(0x9100_0000 | (imm12 << 10) | (rn << 5) | rd)
    }
    /// `sub x<rd>, x<rn>, #imm12`
    pub fn sub_imm(rd: u32, rn: u32, imm12: u32) -> Ins {
        word(0xD100_0000 | (imm12 << 10) | (rn << 5) | rd)
    }
    /// `cmp x<rn>, #imm12` (`subs xzr, xn, #imm12`)
    pub fn cmp_imm(rn: u32, imm12: u32) -> Ins {
        word(0xF100_0000 | (imm12 << 10) | (rn << 5) | 31)
    }
    /// `ldr x<rt>, [x27, #disp]` - an ObjectPool load off the PP register.
    pub fn ldr_pp(rt: u32, disp: u32) -> Ins {
        word(0xF940_0000 | ((disp / 8) << 10) | (27 << 5) | rt)
    }
    /// `stur x<rt>, [x<rn>, #imm9]`
    pub fn stur(rt: u32, rn: u32, imm9: u32) -> Ins {
        word(0xF800_0000 | (imm9 << 12) | (rn << 5) | rt)
    }

    /// Encode a whole program, resolving labels against their own addresses.
    pub fn encode(program: &[Ins]) -> Vec<u32> {
        let mut addresses = std::collections::HashMap::new();
        let mut pc = 0usize;
        for ins in program {
            match ins {
                Ins::Label(name) => {
                    addresses.insert(name.clone(), pc);
                }
                _ => pc += 4,
            }
        }

        let mut out = Vec::new();
        let mut pc = 0i64;
        for ins in program {
            let resolve = |name: &String| -> i64 {
                let target = *addresses
                    .get(name)
                    .unwrap_or_else(|| panic!("undefined label `{name}`"))
                    as i64;
                (target - pc) / 4
            };
            let word = match ins {
                Ins::Label(_) => continue,
                Ins::Word(w) => *w,
                Ins::B(name) => 0x1400_0000 | (resolve(name) as u32 & 0x03FF_FFFF),
                Ins::Bl(name) => 0x9400_0000 | (resolve(name) as u32 & 0x03FF_FFFF),
                Ins::Bc(cond, name) => {
                    0x5400_0000 | ((resolve(name) as u32 & 0x7_FFFF) << 5) | cond
                }
                Ins::Cbz(rt, name) => 0xB400_0000 | ((resolve(name) as u32 & 0x7_FFFF) << 5) | rt,
            };
            out.push(word);
            pc += 4;
        }
        out
    }

    /// Byte offset of each label in the encoded program.
    pub fn offsets(program: &[Ins]) -> std::collections::BTreeMap<String, u64> {
        let mut out = std::collections::BTreeMap::new();
        let mut pc = 0u64;
        for ins in program {
            match ins {
                Ins::Label(name) => {
                    out.insert(name.clone(), pc);
                }
                _ => pc += 4,
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// The synthetic ELF
// ---------------------------------------------------------------------------

/// A minimal AArch64 `ET_DYN` ELF carrying the four snapshot spans the loader
/// reads by symbol name.
///
/// One `PT_LOAD` at vaddr 0 covering the whole file, so a virtual address is its
/// own file offset and `va_to_offset` needs no arithmetic to be checked against.
mod elf {
    pub struct Span {
        pub name: &'static str,
        pub bytes: Vec<u8>,
    }

    pub struct Built {
        pub bytes: Vec<u8>,
        /// Virtual address of each span, in the order they were passed in.
        pub addresses: Vec<u64>,
    }

    fn align_to(out: &mut Vec<u8>, align: usize) {
        while !out.len().is_multiple_of(align) {
            out.push(0);
        }
    }

    pub fn build(spans: &[Span]) -> Built {
        let mut out = vec![0u8; 64 + 56];
        align_to(&mut out, 16);

        let mut addresses = Vec::new();
        for span in spans {
            align_to(&mut out, 16);
            addresses.push(out.len() as u64);
            out.extend_from_slice(&span.bytes);
        }

        // .strtab
        align_to(&mut out, 8);
        let strtab_off = out.len();
        let mut name_offsets = Vec::new();
        out.push(0);
        for span in spans {
            name_offsets.push(out.len() - strtab_off);
            out.extend_from_slice(span.name.as_bytes());
            out.push(0);
        }
        let strtab_size = out.len() - strtab_off;

        // .symtab: one null entry, then one global object per span.
        align_to(&mut out, 8);
        let symtab_off = out.len();
        out.extend_from_slice(&[0u8; 24]);
        for (index, span) in spans.iter().enumerate() {
            out.extend_from_slice(&(name_offsets[index] as u32).to_le_bytes());
            out.push((1 << 4) | 1); // STB_GLOBAL | STT_OBJECT
            out.push(0);
            out.extend_from_slice(&1u16.to_le_bytes()); // any defined section
            out.extend_from_slice(&addresses[index].to_le_bytes());
            out.extend_from_slice(&(span.bytes.len() as u64).to_le_bytes());
        }
        let symtab_size = out.len() - symtab_off;

        // .shstrtab
        align_to(&mut out, 8);
        let shstrtab_off = out.len();
        let mut section_names = Vec::new();
        out.push(0);
        for name in [".symtab", ".strtab", ".shstrtab"] {
            section_names.push(out.len() - shstrtab_off);
            out.extend_from_slice(name.as_bytes());
            out.push(0);
        }
        let shstrtab_size = out.len() - shstrtab_off;

        align_to(&mut out, 8);
        let shoff = out.len();
        let shdr = |name: u32, kind: u32, off: usize, size: usize, link: u32, entsize: u64| {
            let mut h = Vec::with_capacity(64);
            h.extend_from_slice(&name.to_le_bytes());
            h.extend_from_slice(&kind.to_le_bytes());
            h.extend_from_slice(&0u64.to_le_bytes()); // sh_flags
            h.extend_from_slice(&0u64.to_le_bytes()); // sh_addr
            h.extend_from_slice(&(off as u64).to_le_bytes());
            h.extend_from_slice(&(size as u64).to_le_bytes());
            h.extend_from_slice(&link.to_le_bytes());
            h.extend_from_slice(&1u32.to_le_bytes()); // sh_info
            h.extend_from_slice(&8u64.to_le_bytes()); // sh_addralign
            h.extend_from_slice(&entsize.to_le_bytes());
            h
        };
        let headers = [
            shdr(0, 0, 0, 0, 0, 0),
            shdr(section_names[0] as u32, 2, symtab_off, symtab_size, 2, 24),
            shdr(section_names[1] as u32, 3, strtab_off, strtab_size, 0, 0),
            shdr(
                section_names[2] as u32,
                3,
                shstrtab_off,
                shstrtab_size,
                0,
                0,
            ),
        ];
        for header in &headers {
            out.extend_from_slice(header);
        }

        let total = out.len() as u64;

        // ELF header.
        let mut header = Vec::with_capacity(64);
        header.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0]);
        header.extend_from_slice(&[0u8; 8]);
        header.extend_from_slice(&3u16.to_le_bytes()); // ET_DYN
        header.extend_from_slice(&183u16.to_le_bytes()); // EM_AARCH64
        header.extend_from_slice(&1u32.to_le_bytes());
        header.extend_from_slice(&0u64.to_le_bytes()); // e_entry
        header.extend_from_slice(&64u64.to_le_bytes()); // e_phoff
        header.extend_from_slice(&(shoff as u64).to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes()); // e_flags
        header.extend_from_slice(&64u16.to_le_bytes()); // e_ehsize
        header.extend_from_slice(&56u16.to_le_bytes()); // e_phentsize
        header.extend_from_slice(&1u16.to_le_bytes()); // e_phnum
        header.extend_from_slice(&64u16.to_le_bytes()); // e_shentsize
        header.extend_from_slice(&(headers.len() as u16).to_le_bytes());
        header.extend_from_slice(&3u16.to_le_bytes()); // e_shstrndx
        out[..64].copy_from_slice(&header);

        // The single PT_LOAD, vaddr 0, covering everything written above.
        let mut phdr = Vec::with_capacity(56);
        phdr.extend_from_slice(&1u32.to_le_bytes()); // PT_LOAD
        phdr.extend_from_slice(&5u32.to_le_bytes()); // R+X
        phdr.extend_from_slice(&0u64.to_le_bytes()); // p_offset
        phdr.extend_from_slice(&0u64.to_le_bytes()); // p_vaddr
        phdr.extend_from_slice(&0u64.to_le_bytes()); // p_paddr
        phdr.extend_from_slice(&total.to_le_bytes());
        phdr.extend_from_slice(&total.to_le_bytes());
        phdr.extend_from_slice(&0x1000u64.to_le_bytes());
        out[64..120].copy_from_slice(&phdr);

        Built {
            bytes: out,
            addresses,
        }
    }
}

// ---------------------------------------------------------------------------
// The synthetic program
// ---------------------------------------------------------------------------

/// Every emitter shape the determinism contract names, in one program.
///
/// Keeping them in one input means every one of the twenty runs carries all
/// five, rather than five separate inputs each carrying one.
mod fixture {
    use super::asm::{self, Ins};

    /// How long a chain of conditional blocks the helper-cap function carries.
    ///
    /// The DFS fallback inlines twelve levels deep and hands the thirteenth to a
    /// block helper, which starts its own walk twelve levels deeper again, so the
    /// chain buys roughly one helper definition per twelve blocks. The emitter
    /// defines at most sixty-four of them; a fixture that never crosses the cap
    /// it claims to test is not coverage for that cap, so this is sized to clear
    /// it with room to spare.
    pub const HELPER_CAP_CHAIN: usize = 900;
    /// How many independent chains that length is split across.
    pub const HELPER_CAP_BRANCHES: usize = 3;

    /// ObjectPool entry index whose selector names the aliased callee.
    pub const SELECTOR_POOL_INDEX: u64 = 3;
    /// The PP displacement that resolves to it under the declared geometry.
    pub fn selector_pool_displacement() -> u32 {
        (POOL_ENTRIES_OFFSET + POOL_WORD_SIZE * SELECTOR_POOL_INDEX) as u32
    }
    pub fn pool_displacement(index: u64) -> u32 {
        (POOL_ENTRIES_OFFSET + POOL_WORD_SIZE * index) as u32
    }
    pub const POOL_ENTRIES_OFFSET: u64 = 0x10;
    pub const POOL_WORD_SIZE: u64 = 8;
    pub const RECOVERED_BAIT_INDEX: u64 = 4;
    pub const COLLIDING_SELECTOR_INDEX: u64 = 10;
    pub const RECOVERED_BAIT: &str = "x = sub_1110(y) } ${ arg0 x28 _block_999 */ //";

    /// A join reached from two arms that bind the same register differently.
    /// Its first `0x11` value is also a deliberate liveness-filter rejection:
    /// it resembles the absent register token `x11`, while the duplicate-line
    /// fixture below supplies an accepted provenance candidate.
    fn ambiguous_fan_in() -> Vec<Ins> {
        vec![
            asm::label("fanIn"),
            asm::prologue(),
            asm::movz(0, 1),
            asm::cmp_imm(0, 0),
            asm::bc(asm::COND_EQ, "fanIn.else"),
            asm::movz(9, 0x11),
            asm::b("fanIn.join"),
            asm::label("fanIn.else"),
            asm::movz(9, 0x22),
            asm::label("fanIn.join"),
            asm::mov(2, 9),
            asm::stur(9, 19, 7),
            asm::epilogue(),
            asm::ret(),
            asm::label("fanIn.end"),
        ]
    }

    /// A counted loop: a header reached both from outside and from its own
    /// latch, so the header's incoming value is a loop-entry merge.
    fn counted_loop() -> Vec<Ins> {
        vec![
            asm::label("loop"),
            asm::prologue(),
            asm::movz(0, 0),
            asm::movz(9, 3),
            asm::label("loop.top"),
            asm::add_imm(0, 0, 1),
            asm::mov(2, 9),
            asm::movz(9, 5),
            asm::cmp_imm(0, 4),
            asm::bc(asm::COND_NE, "loop.top"),
            asm::mov(3, 9),
            asm::epilogue(),
            asm::ret(),
            asm::label("loop.end"),
        ]
    }

    /// Two rendered lines that are byte-identical, one of them at a join whose
    /// value is ambiguous. The annotation has to land on its own line and not on
    /// the earlier twin.
    fn duplicate_line_provenance() -> Vec<Ins> {
        vec![
            asm::label("dupLine"),
            asm::prologue(),
            asm::stur(9, 19, 7),
            asm::cbz(1, "dupLine.else"),
            asm::movz(9, 7),
            asm::b("dupLine.join"),
            asm::label("dupLine.else"),
            asm::movz(9, 9),
            asm::label("dupLine.join"),
            asm::stur(9, 19, 7),
            asm::epilogue(),
            asm::ret(),
            asm::label("dupLine.end"),
        ]
    }

    /// An irreducible two-entry cycle, which region analysis refuses, a few
    /// diamonds, then a long chain of conditional blocks.
    ///
    /// The refusal forces the whole function down the DFS fallback. There the
    /// chain outruns the depth budget over and over, and each block the walk
    /// cannot reach becomes a block helper whose own walk runs out of depth
    /// twelve blocks later, until the helper budget itself is exhausted.
    fn helper_cap() -> Vec<Ins> {
        let mut out = vec![
            asm::label("helperCap"),
            asm::prologue(),
            asm::movz(0, 0),
            asm::cmp_imm(0, 0),
            asm::bc(asm::COND_EQ, "helperCap.c"),
            asm::label("helperCap.b"),
            asm::add_imm(0, 0, 1),
            asm::b("helperCap.c"),
            asm::label("helperCap.c"),
            asm::cmp_imm(0, 9),
            asm::bc(asm::COND_LT, "helperCap.b"),
        ];
        // A handful of diamonds first, so the declined function also carries
        // join blocks with more than one predecessor and not only a chain.
        for index in 0..6u32 {
            let taken = format!("helperCap.d{index}.taken");
            let join = format!("helperCap.d{index}.join");
            out.push(asm::cmp_imm(0, index + 1));
            out.push(asm::bc(asm::COND_EQ, &taken));
            out.push(asm::movz(1, index + 1));
            out.push(asm::b(&join));
            out.push(asm::label(&taken));
            out.push(asm::movz(1, index + 2));
            out.push(asm::label(&join));
            out.push(asm::add_imm(0, 0, 1));
        }
        // Three independent chains rather than one, so the walk is owing several
        // block helpers at the same time and the order it defines them in is a
        // property of the queue and not of a single chain.
        for branch in 0..HELPER_CAP_BRANCHES {
            out.push(asm::cbz(0, &format!("helperCap.chain{branch}")));
        }
        out.push(asm::b("helperCap.tail"));
        for branch in 0..HELPER_CAP_BRANCHES {
            out.push(asm::label(&format!("helperCap.chain{branch}")));
            for step in 0..HELPER_CAP_CHAIN / HELPER_CAP_BRANCHES {
                out.push(asm::cbz(0, "helperCap.tail"));
                // Past the first helper boundary, put unmatched recovered braces
                // and rewrite bait in a helper body, not only in the outer body.
                if branch == 0 && step == 24 {
                    out.push(asm::ldr_pp(7, pool_displacement(RECOVERED_BAIT_INDEX)));
                    out.push(asm::stur(7, 19, 7));
                }
            }
            out.push(asm::b("helperCap.tail"));
        }
        out.push(asm::label("helperCap.tail"));
        out.push(asm::epilogue());
        out.push(asm::ret());
        out.push(asm::label("helperCap.end"));
        out
    }

    /// Two calls to the same placeholder-named callee whose argument register
    /// carries a pool slot the model tags with a selector, so both are renamed
    /// and leave `was: sub_...` evidence behind.
    fn alias_evidence() -> Vec<Ins> {
        vec![
            asm::label("aliasEvidence"),
            asm::prologue(),
            asm::ldr_pp(1, selector_pool_displacement()),
            asm::ldr_pp(2, pool_displacement(4)),
            asm::bl("aliasTarget"),
            asm::mov(3, 0),
            asm::ldr_pp(1, selector_pool_displacement()),
            asm::bl("aliasTarget"),
            asm::mov(4, 0),
            asm::epilogue(),
            asm::ret(),
            asm::label("aliasEvidence.end"),
        ]
    }

    /// A third call to the same callee with no pool evidence of its own. The
    /// program-level generic-call pass has to reach it from the other function's
    /// evidence, which is what makes alias order a whole-program property.
    fn alias_consumer() -> Vec<Ins> {
        vec![
            asm::label("aliasConsumer"),
            asm::prologue(),
            asm::movz(1, 0x31),
            asm::bl("aliasTarget"),
            asm::mov(5, 0),
            asm::epilogue(),
            asm::ret(),
            asm::label("aliasConsumer.end"),
        ]
    }

    /// The callee itself, left with a placeholder name in the model.
    fn alias_target() -> Vec<Ins> {
        vec![
            asm::label("aliasTarget"),
            asm::prologue(),
            asm::add_imm(0, 1, 4),
            asm::epilogue(),
            asm::ret(),
            asm::label("aliasTarget.end"),
        ]
    }

    fn string_victim() -> Vec<Ins> {
        vec![
            asm::label("stringVictim"),
            asm::ldr_pp(0, pool_displacement(4)),
            asm::ret(),
            asm::label("stringVictim.end"),
        ]
    }

    /// A wide producer followed by a narrow consumer. Substituting the `x8`
    /// expression through the `w9` read would claim the high half survived.
    fn width_boundary() -> Vec<Ins> {
        vec![
            asm::label("widthBoundary"),
            asm::prologue(),
            asm::mov(9, 8),
            asm::mov_w(10, 9),
            asm::stur(10, 19, 7),
            asm::epilogue(),
            asm::ret(),
            asm::label("widthBoundary.end"),
        ]
    }

    /// Recovered selector text collides with the generated helper namespace.
    fn helper_namespace_collision() -> Vec<Ins> {
        vec![
            asm::label("helperNamespaceCollision"),
            asm::prologue(),
            asm::ldr_pp(1, pool_displacement(COLLIDING_SELECTOR_INDEX)),
            asm::bl("collisionTarget"),
            asm::mov(5, 0),
            asm::epilogue(),
            asm::ret(),
            asm::label("helperNamespaceCollision.end"),
        ]
    }

    fn collision_target() -> Vec<Ins> {
        vec![
            asm::label("collisionTarget"),
            asm::prologue(),
            asm::add_imm(0, 1, 1),
            asm::epilogue(),
            asm::ret(),
            asm::label("collisionTarget.end"),
        ]
    }

    /// Two unresolved registers each decremented the same number of times, so
    /// both clear the aliasing threshold with an identical count.
    ///
    /// The alias candidates arrive from a `HashMap` and are ranked by frequency;
    /// a tie is broken by name. Without that second key the two `final int
    /// regNMinus1` declarations swap places between processes while every
    /// counter stays identical, which is the drift that only a whole-artifact
    /// comparison sees.
    fn alias_order() -> Vec<Ins> {
        let mut out = vec![asm::label("aliasOrder"), asm::prologue()];
        for step in 0..4u32 {
            for (index, source) in [8u32, 9u32].into_iter().enumerate() {
                let dest = 10 + step * 2 + index as u32;
                out.push(asm::sub_imm(dest, source, 1));
                out.push(asm::stur(dest, 19, 0x10 + (step * 2 + index as u32) * 8));
            }
        }
        out.push(asm::epilogue());
        out.push(asm::ret());
        out.push(asm::label("aliasOrder.end"));
        out
    }

    /// Calls a placeholder-named function that two ObjectPool entries both claim,
    /// with different owners and selectors.
    ///
    /// Which of the two names the function is emitted under is decided by the
    /// pool index, so a reader of that map that iterates it unordered picks the
    /// name from the process's hash seed.
    fn pool_named_caller() -> Vec<Ins> {
        vec![
            asm::label("poolNamedCaller"),
            asm::prologue(),
            asm::movz(1, 0x41),
            asm::bl("poolTarget"),
            asm::mov(6, 0),
            asm::epilogue(),
            asm::ret(),
            asm::label("poolNamedCaller.end"),
        ]
    }

    fn pool_named_target() -> Vec<Ins> {
        vec![
            asm::label("poolTarget"),
            asm::prologue(),
            asm::add_imm(0, 1, 8),
            asm::epilogue(),
            asm::ret(),
            asm::label("poolTarget.end"),
        ]
    }

    /// The two pool entries that claim `poolTarget`, lowest index first.
    pub const POOL_TIE_INDEXES: (u64, u64) = (5, 9);
    /// The symbol the lowest-index claim produces, and the one the other claim
    /// would produce if map order decided it.
    pub const POOL_TIE_WINNER: &str = "flutter.widgets.Alpha.ping";
    pub const POOL_TIE_LOSER: &str = "flutter.widgets.Omega.pong";

    /// Label of each fixture function's entry and one past its last word, in the
    /// order the functions appear in the blob.
    pub const FUNCTIONS: [(&str, &str, &str); 14] = [
        ("fanIn", "fanIn.end", "fanInJoin"),
        ("loop", "loop.end", "countedLoop"),
        ("dupLine", "dupLine.end", "duplicateLineProvenance"),
        ("helperCap", "helperCap.end", "helperBudgetChain"),
        ("aliasEvidence", "aliasEvidence.end", "aliasEvidenceSite"),
        ("aliasConsumer", "aliasConsumer.end", "aliasConsumerSite"),
        // Placeholder-named on purpose: the generic-call pass only aliases a
        // callee whose own name is a placeholder.
        ("aliasTarget", "aliasTarget.end", ""),
        ("stringVictim", "stringVictim.end", "stringVictim"),
        ("widthBoundary", "widthBoundary.end", "widthBoundary"),
        (
            "helperNamespaceCollision",
            "helperNamespaceCollision.end",
            "helperNamespaceCollision",
        ),
        // Placeholder-named so the recovered `_block_999` selector renames it.
        ("collisionTarget", "collisionTarget.end", ""),
        ("poolNamedCaller", "poolNamedCaller.end", "poolNamedCaller"),
        // Also placeholder-named, so the pool claims decide its symbol.
        ("poolTarget", "poolTarget.end", ""),
        ("aliasOrder", "aliasOrder.end", "aliasOrderSite"),
    ];

    pub fn program() -> Vec<Ins> {
        let mut out = Vec::new();
        out.extend(ambiguous_fan_in());
        out.extend(counted_loop());
        out.extend(duplicate_line_provenance());
        out.extend(helper_cap());
        out.extend(alias_evidence());
        out.extend(alias_consumer());
        out.extend(alias_target());
        out.extend(string_victim());
        out.extend(width_boundary());
        out.extend(helper_namespace_collision());
        out.extend(collision_target());
        out.extend(pool_named_caller());
        out.extend(pool_named_target());
        out.extend(alias_order());
        out
    }
}

// ---------------------------------------------------------------------------
// Raw JSON scalar location
// ---------------------------------------------------------------------------

/// Locating exact byte ranges of named scalars in a raw JSON document, without
/// building a value out of it.
///
/// `serde_json` would happily read `report.json` and write it back, but a parse
/// and re-serialize normalizes key order, number formatting and escaping, which
/// is the drift the comparison is looking for. This walks the raw bytes instead
/// and reports where each frozen scalar starts and ends, so everything around it
/// can be compared as the bytes that were actually written.
mod raw_json {
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    pub enum Scalar {
        String,
        Number,
        Bool,
        Null,
    }

    #[derive(Debug, Clone)]
    pub struct Located {
        pub pointer: String,
        pub start: usize,
        pub end: usize,
        pub kind: Scalar,
    }

    /// Does a concrete JSON pointer match a pattern whose `*` segments stand for
    /// any single array index?
    fn matches(pattern: &str, pointer: &str) -> bool {
        let mut p = pattern.split('/');
        let mut q = pointer.split('/');
        loop {
            match (p.next(), q.next()) {
                (None, None) => return true,
                (Some(a), Some(b)) if a == "*" || a == b => continue,
                _ => return false,
            }
        }
    }

    struct Scanner<'a> {
        bytes: &'a [u8],
        at: usize,
        patterns: &'a [&'a str],
        found: Vec<Located>,
    }

    impl<'a> Scanner<'a> {
        fn skip_ws(&mut self) {
            while self
                .bytes
                .get(self.at)
                .is_some_and(|b| matches!(b, b' ' | b'\n' | b'\r' | b'\t'))
            {
                self.at += 1;
            }
        }

        fn expect(&mut self, byte: u8) -> Result<(), String> {
            self.skip_ws();
            if self.bytes.get(self.at) == Some(&byte) {
                self.at += 1;
                Ok(())
            } else {
                Err(format!("expected `{}` at byte {}", byte as char, self.at))
            }
        }

        /// Consume a string and return its unescaped contents.
        fn string(&mut self) -> Result<String, String> {
            self.expect(b'"')?;
            let mut out = String::new();
            loop {
                let byte = *self
                    .bytes
                    .get(self.at)
                    .ok_or_else(|| "unterminated string".to_string())?;
                self.at += 1;
                match byte {
                    b'"' => return Ok(out),
                    b'\\' => {
                        let escape = *self
                            .bytes
                            .get(self.at)
                            .ok_or_else(|| "unterminated escape".to_string())?;
                        self.at += 1;
                        match escape {
                            b'u' => {
                                let hex = self
                                    .bytes
                                    .get(self.at..self.at + 4)
                                    .ok_or_else(|| "short \\u escape".to_string())?;
                                let code = u32::from_str_radix(
                                    std::str::from_utf8(hex).map_err(|e| e.to_string())?,
                                    16,
                                )
                                .map_err(|e| e.to_string())?;
                                self.at += 4;
                                out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                            }
                            b'n' => out.push('\n'),
                            b't' => out.push('\t'),
                            b'r' => out.push('\r'),
                            b'b' => out.push('\u{8}'),
                            b'f' => out.push('\u{c}'),
                            other => out.push(other as char),
                        }
                    }
                    other => out.push(other as char),
                }
            }
        }

        fn value(&mut self, pointer: &str) -> Result<(), String> {
            self.skip_ws();
            let start = self.at;
            let kind = match self.bytes.get(self.at) {
                Some(b'{') => {
                    self.at += 1;
                    self.skip_ws();
                    if self.bytes.get(self.at) == Some(&b'}') {
                        self.at += 1;
                        return Ok(());
                    }
                    loop {
                        self.skip_ws();
                        let key = self.string()?;
                        self.expect(b':')?;
                        self.value(&format!("{pointer}/{}", escape_token(&key)))?;
                        self.skip_ws();
                        match self.bytes.get(self.at) {
                            Some(b',') => self.at += 1,
                            Some(b'}') => {
                                self.at += 1;
                                return Ok(());
                            }
                            _ => return Err(format!("bad object at byte {}", self.at)),
                        }
                    }
                }
                Some(b'[') => {
                    self.at += 1;
                    self.skip_ws();
                    if self.bytes.get(self.at) == Some(&b']') {
                        self.at += 1;
                        return Ok(());
                    }
                    let mut index = 0usize;
                    loop {
                        self.value(&format!("{pointer}/{index}"))?;
                        index += 1;
                        self.skip_ws();
                        match self.bytes.get(self.at) {
                            Some(b',') => self.at += 1,
                            Some(b']') => {
                                self.at += 1;
                                return Ok(());
                            }
                            _ => return Err(format!("bad array at byte {}", self.at)),
                        }
                    }
                }
                Some(b'"') => {
                    self.string()?;
                    Scalar::String
                }
                Some(b't') => {
                    self.at += 4;
                    Scalar::Bool
                }
                Some(b'f') => {
                    self.at += 5;
                    Scalar::Bool
                }
                Some(b'n') => {
                    self.at += 4;
                    Scalar::Null
                }
                Some(_) => {
                    while self.bytes.get(self.at).is_some_and(|b| {
                        b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E')
                    }) {
                        self.at += 1;
                    }
                    Scalar::Number
                }
                None => return Err("unexpected end of document".to_string()),
            };

            if self.patterns.iter().any(|p| matches(p, pointer)) {
                self.found.push(Located {
                    pointer: pointer.to_string(),
                    start,
                    end: self.at,
                    kind,
                });
            }
            Ok(())
        }
    }

    fn escape_token(key: &str) -> String {
        key.replace('~', "~0").replace('/', "~1")
    }

    /// Every scalar whose pointer matches one of `patterns`, in document order.
    pub fn locate(bytes: &[u8], patterns: &[&str]) -> Result<Vec<Located>, String> {
        let mut scanner = Scanner {
            bytes,
            at: 0,
            patterns,
            found: Vec::new(),
        };
        scanner.value("")?;
        scanner.skip_ws();
        if scanner.at != bytes.len() {
            return Err(format!(
                "trailing bytes after the document at {}",
                scanner.at
            ));
        }
        Ok(scanner.found)
    }

    /// The raw slices between and around `spans`, in order.
    pub fn untouched<'a>(bytes: &'a [u8], spans: &[Located]) -> Vec<&'a [u8]> {
        let mut out = Vec::new();
        let mut cursor = 0usize;
        for span in spans {
            out.push(&bytes[cursor..span.start]);
            cursor = span.end;
        }
        out.push(&bytes[cursor..]);
        out
    }
}

// ---------------------------------------------------------------------------
// The frozen volatile pointer list
// ---------------------------------------------------------------------------

/// `docs/oracle-protocol-ir-cfg-emitter.md` section 6, verbatim and complete.
///
/// Absolute output paths only. Nothing may be added here after a comparison has
/// been attempted, so a byte difference outside these is a determinism failure
/// and never a reason to grow the list.
const FROZEN_POINTERS: [&str; 9] = [
    "/input",
    "/libapp",
    "/adapter_selection/adapter_exec_path",
    "/extra_symbol_elfs/*",
    "/extra_symbol_map_targets/*",
    "/engine_symbol_ingestion/manifest_path",
    "/engine_symbol_ingestion/loaded_paths/*",
    "/ghidra_script/path",
    "/ida_script/path",
];

/// The frozen pointers that must actually be present as strings in this
/// fixture's `report.json`.
///
/// Without this the comparison could pass while locating nothing at all, which
/// is byte equality with extra steps rather than the tolerance the contract asks
/// for.
const REQUIRED_VOLATILE: [&str; 5] = [
    "/input",
    "/libapp",
    "/adapter_selection/adapter_exec_path",
    "/ghidra_script/path",
    "/ida_script/path",
];

// ---------------------------------------------------------------------------
// One pipeline run
// ---------------------------------------------------------------------------

fn options(out_dir: PathBuf) -> DecompileOptions {
    DecompileOptions {
        out_dir,
        emit_asm: false,
        emit_asm_opcodes: false,
        // On, so the two script paths exist and the volatile-scalar machinery is
        // exercised against a real length difference rather than nothing.
        emit_ghidra_script: true,
        emit_ida_script: true,
        emit_ir: true,
        split_records: false,
        extra_symbol_elfs: Vec::new(),
        extra_symbol_map_targets: Vec::new(),
        include_nearest_symbol_map: false,
        focus: None,
        function_target: None,
        max_functions: None,
        // The CLI's defaults. The synthetic program does not clear them, so the
        // gate verdict lands in `quality.json` and is compared like everything
        // else rather than being tuned away.
        max_placeholder_ifs: 0,
        max_unresolved_cf: 0,
        max_indirect_call_ratio: 0.30,
        min_disassembly_ratio: 0.80,
        function_scope: FunctionScope::All,
        app_packages: Vec::new(),
        adapter_backend: AdapterBackend::Internal,
        require_snapshot_hash_match: false,
        analysis_profile: DecompileAnalysisProfile::Balanced,
        engine_options: DecompileEngineOptions::for_profile(DecompileAnalysisProfile::Balanced),
    }
}

/// A Dart snapshot header: magic, length, kind, the 32-character hash, then the
/// NUL-terminated features string. The loader reads the hash out of this rather
/// than scanning, so the bundle's hash is exactly what the adapter is installed
/// under.
fn snapshot_data(tail: &str) -> Vec<u8> {
    let features = b"product no-code_comments no-compressed-pointers\0";
    let mut out = Vec::new();
    out.extend_from_slice(&[0xf5, 0xf5, 0xdc, 0xdc]);
    let payload = 12 + 32 + features.len() + tail.len();
    out.extend_from_slice(&(payload as i64).to_le_bytes());
    out.extend_from_slice(&0i64.to_le_bytes());
    out.extend_from_slice(SNAPSHOT_HASH.as_bytes());
    out.extend_from_slice(features);
    out.extend_from_slice(tail.as_bytes());
    out
}

/// Everything a child needs on disk: the synthetic `libapp.so`, an adapter
/// installed for its snapshot hash, and the model that adapter hands back.
fn plant_workspace(root: &Path) -> PathBuf {
    let program = fixture::program();
    let words = asm::encode(&program);
    let offsets = asm::offsets(&program);
    let code: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();

    let built = elf::build(&[
        elf::Span {
            name: "_kDartVmSnapshotData",
            bytes: snapshot_data(" vm"),
        },
        elf::Span {
            name: "_kDartIsolateSnapshotData",
            bytes: snapshot_data(" isolate"),
        },
        elf::Span {
            name: "_kDartVmSnapshotInstructions",
            bytes: vec![0xc0, 0x03, 0x5f, 0xd6],
        },
        elf::Span {
            name: "_kDartIsolateSnapshotInstructions",
            bytes: code,
        },
    ]);
    let code_va = built.addresses[3];

    let libapp = root.join("libapp.so");
    fs::write(&libapp, &built.bytes).expect("write synthetic libapp");

    let functions = fixture::FUNCTIONS
        .iter()
        .enumerate()
        .map(|(id, (entry, end, name))| {
            let entry_va = code_va + offsets[*entry];
            let size = offsets[*end] - offsets[*entry];
            let name = if name.is_empty() {
                format!("sub_{entry_va:x}")
            } else {
                (*name).to_string()
            };
            serde_json::json!({
                "id": id,
                "name": name,
                "owner_class": if name.starts_with("sub_") { "Global" } else { "AppState" },
                "entry_va": entry_va,
                "size": size,
                "code_section_va": code_va,
                "name_kind": if name.starts_with("sub_") { "placeholder" } else { "exact" }
            })
        })
        .collect::<Vec<_>>();
    let pool_target_va = code_va + offsets["poolTarget"];

    let model = serde_json::json!({
        "schema_version": 3,
        // Contains "internal", so the resolved backend matches the requested one
        // and no compatibility warning is raised for the backend.
        "adapter_kind": "synthetic-internal",
        "dart_version": "3.9.2",
        "snapshot_hash": SNAPSHOT_HASH,
        "arch": "arm64",
        "libraries": [
            { "id": 0, "uri": "package:app/main.dart", "name_display": "package:app/main.dart" },
            {
                "id": 1,
                "uri": "package:flutter/src/widgets/framework.dart",
                "name_display": "package:flutter/src/widgets/framework.dart"
            }
        ],
        "classes": [
            { "id": 0, "name": "Global", "super": "Object", "lib": "package:app/main.dart" },
            {
                "id": 1,
                "name": "AppState",
                "super": "State",
                "lib": "package:flutter/src/widgets/framework.dart"
            }
        ],
        "functions": functions,
        "object_pool": [
            { "index": 0, "kind": "string", "value": "app/main.dart" },
            { "index": 1, "kind": "int", "value": "42" },
            { "index": 2, "kind": "string", "value": "AppState" },
            {
                "index": fixture::SELECTOR_POOL_INDEX,
                "kind": "string",
                "value": "arg0",
                "decoded_kind": "selector",
                "selector": "arg0",
                "owner_class": "AppState",
                "library_uri": "package:flutter/src/widgets/framework.dart",
                "confidence": 0.9,
                "source": "synthetic"
            },
            {
                "index": fixture::RECOVERED_BAIT_INDEX,
                "kind": "string",
                "value": fixture::RECOVERED_BAIT
            },
            // Two entries claiming one target, with different owners and
            // selectors. The lower index has to win, whatever order the hint map
            // happens to iterate in.
            {
                "index": fixture::POOL_TIE_INDEXES.0,
                "kind": "string",
                "value": "ping",
                "decoded_kind": "selector",
                "selector": "ping",
                "owner_class": "Alpha",
                "library_uri": "package:flutter/src/widgets/framework.dart",
                "target_va": pool_target_va,
                "source": "synthetic"
            },
            {
                "index": fixture::POOL_TIE_INDEXES.1,
                "kind": "string",
                "value": "pong",
                "decoded_kind": "selector",
                "selector": "pong",
                "owner_class": "Omega",
                "library_uri": "package:flutter/src/widgets/framework.dart",
                "target_va": pool_target_va,
                "source": "synthetic"
            },
            {
                "index": fixture::COLLIDING_SELECTOR_INDEX,
                "kind": "string",
                "value": "_block_999",
                "decoded_kind": "selector",
                "selector": "_block_999",
                "owner_class": "AppState",
                "library_uri": "package:flutter/src/widgets/framework.dart",
                "confidence": 0.9,
                "source": "synthetic"
            }
        ],
        "pool_geometry": {
            "entries_offset": fixture::POOL_ENTRIES_OFFSET,
            "word_size": fixture::POOL_WORD_SIZE
        }
    });
    fs::write(
        root.join("model.json"),
        serde_json::to_vec_pretty(&model).expect("model is serializable"),
    )
    .expect("write model");

    // An adapter is an executable that turns the snapshot spans into a model.
    // This one hands back the model authored above, which is what makes the
    // program under test synthetic and fully specified rather than carved.
    fs::create_dir_all(root.join("adapters/installed")).expect("mkdir adapters");
    fs::write(
        root.join("adapters/manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "entries": [{
                "snapshot_hash": SNAPSHOT_HASH,
                "version": "synthetic",
                "adapter": format!("dart_adapter_{SNAPSHOT_HASH}")
            }]
        }))
        .expect("manifest is serializable"),
    )
    .expect("write manifest");
    // `find_repo_root` wants both markers, so the same directory also works as a
    // working directory for a hand-run CLI invocation.
    fs::write(
        root.join("Cargo.toml"),
        b"# synthetic pipeline fixture root\n",
    )
    .expect("write root marker");

    let adapter = root.join(format!("adapters/installed/dart_adapter_{SNAPSHOT_HASH}"));
    fs::write(
        &adapter,
        b"#!/usr/bin/env python3\n\
          import pathlib, shutil, sys\n\
          argv = sys.argv[1:]\n\
          out = argv[argv.index('--out') + 1]\n\
          root = pathlib.Path(__file__).resolve().parents[2]\n\
          shutil.copyfile(str(root / 'model.json'), out)\n" as &[u8],
    )
    .expect("write adapter");
    let mut perms = fs::metadata(&adapter)
        .expect("adapter metadata")
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    fs::set_permissions(&adapter, perms).expect("chmod adapter");

    libapp
}

/// A value decided by `HashSet` iteration order and nothing else.
fn hash_order_canary() -> String {
    let members: std::collections::HashSet<usize> = (0..32).collect();
    members
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

// ---------------------------------------------------------------------------
// Reading a run back
// ---------------------------------------------------------------------------

/// Every file under `dir`, relative path and bytes, in sorted path order.
fn read_tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|e| e.expect("dir entry").path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path
            .file_name()
            .expect("named entry")
            .to_string_lossy()
            .to_string();
        if path.is_dir() {
            for (child, bytes) in read_tree(&path) {
                out.push((format!("{name}/{child}"), bytes));
            }
        } else {
            out.push((name, fs::read(&path).expect("read artifact")));
        }
    }
    out
}

#[derive(Clone)]
struct Run {
    /// Everything the run wrote except the two JSON reports: pseudocode, emitted
    /// IR and the symbol scripts, relative path and bytes.
    artifacts: Vec<(String, Vec<u8>)>,
    quality: Vec<u8>,
    report: Vec<u8>,
}

fn read_run(out_dir: &Path) -> Run {
    let all = read_tree(out_dir);
    let artifacts = all
        .iter()
        .filter(|(name, _)| name != "quality.json" && name != "report.json")
        .cloned()
        .collect::<Vec<_>>();
    let pick = |name: &str| {
        all.iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{name} missing from {}", out_dir.display()))
            .1
            .clone()
    };
    Run {
        artifacts,
        quality: pick("quality.json"),
        report: pick("report.json"),
    }
}

/// The fixture's own coverage check: every shape the contract names has to be
/// visible in the artifacts, or the twenty runs agree about nothing in
/// particular.
fn assert_fixture_covers_every_shape(run: &Run) {
    let quality: serde_json::Value =
        serde_json::from_slice(&run.quality).expect("quality.json parses");
    let sources = run
        .artifacts
        .iter()
        .filter(|(name, _)| name.starts_with("pseudocode/"))
        .map(|(_, bytes)| String::from_utf8_lossy(bytes).to_string())
        .collect::<Vec<_>>();
    let pseudocode = sources.join("\n");

    let count = |pointer: &str| -> u64 {
        quality
            .pointer(pointer)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_else(|| panic!("quality.json has no counter at {pointer}"))
    };

    let raw_args = (0..=7)
        .map(|n| {
            pseudocode
                .lines()
                .map(|line| {
                    flutterdec_decompiler::count_code_identifier_tokens(line, &format!("arg{n}"))
                })
                .sum::<usize>()
        })
        .sum::<usize>() as u64;
    let raw_registers = (0..=30)
        .map(|n| {
            pseudocode
                .lines()
                .map(|line| {
                    flutterdec_decompiler::count_code_identifier_tokens(line, &format!("x{n}"))
                        + flutterdec_decompiler::count_code_identifier_tokens(
                            line,
                            &format!("reg{n}"),
                        )
                })
                .sum::<usize>()
        })
        .sum::<usize>() as u64;
    let helper_refs = pseudocode
        .lines()
        .map(|line| flutterdec_decompiler::count_code_matches(line, "_block_"))
        .sum::<usize>() as u64;
    let helper_source = sources
        .iter()
        .find(|source| source.contains("helperBudgetChain"))
        .expect("helper-cap artifact is missing");
    let (helper_calls, helper_definitions) = helper_symbols(helper_source);
    let call_set: BTreeSet<&str> = helper_calls.iter().map(String::as_str).collect();
    let definition_set: BTreeSet<&str> = helper_definitions.iter().map(String::as_str).collect();
    assert_eq!(count("/raw_arg_name_refs"), raw_args);
    assert_eq!(count("/raw_register_name_refs"), raw_registers);
    assert_eq!(count("/block_helper_refs"), helper_refs);
    assert_eq!(
        call_set, definition_set,
        "helper calls and definitions differ"
    );
    assert_eq!(
        helper_definitions.len(),
        definition_set.len(),
        "a helper was defined more than once"
    );
    assert_eq!(
        helper_refs,
        (helper_calls.len() + helper_definitions.len()) as u64,
        "recovered data was counted as helper syntax"
    );
    assert!(
        pseudocode.contains("flutter.widgets.AppState.recovered_arg0("),
        "the recovered selector did not stay disjoint from the slot0 local"
    );
    assert!(
        pseudocode.contains("intent: \"framework:flutter.widgets.AppState.arg0 [selector]\""),
        "the recovered selector's exact decoded spelling is not retained"
    );
    assert!(
        !pseudocode.contains("AppState.slot0("),
        "the local rename entered the recovered selector"
    );
    assert!(
        pseudocode.contains(
            "\"x = sub_1110(y) } \\${ arg0 x28 _block_999 *\\u{2f} \\u{2f}/\" /* pool[4] */"
        ),
        "the recovered literal did not retain its exact decoded value"
    );
    assert!(
        !pseudocode.contains("\"x = flutter."),
        "the generic-call rewrite entered recovered literal bait"
    );
    let bait_line = helper_source
        .lines()
        .position(|line| line.contains("x = sub_1110(y) }"))
        .expect("brace-bearing recovered data is absent from the helper fixture");
    let helper_line = helper_source
        .lines()
        .take(bait_line)
        .enumerate()
        .filter_map(|(index, line)| {
            line.trim_start()
                .starts_with("dynamic _block_")
                .then_some(index)
        })
        .last()
        .expect("brace-bearing recovered data stayed outside every helper body");
    assert!(
        helper_line < bait_line,
        "the brace-bearing recovered data is not inside a helper"
    );
    assert!(
        pseudocode.contains("flutter.widgets.AppState.block_999("),
        "the helper-namespace collision did not use a disjoint recovered name"
    );
    assert!(
        pseudocode.contains("\"_block_999\" /* pool[10] */"),
        "the colliding selector's exact recovered spelling is absent"
    );
    assert!(
        !pseudocode.contains("AppState._block_999("),
        "recovered selector text entered the generated helper namespace"
    );
    assert!(
        sources.iter().any(|source| {
            source.contains("widthBoundary")
                && source.contains("reg9")
                && !source.contains("= reg8;")
        }),
        "the x-producer to w-consumer boundary was not preserved"
    );

    assert!(
        pseudocode.contains(" /* = ") || pseudocode.contains(" /* possible (non-exhaustive): "),
        "no join annotation: the ambiguous fan-in shape is not covered"
    );
    assert!(
        count("/loop_backedge_markers") > 0,
        "no loop back-edge marker: the loop shape is not covered"
    );
    assert!(
        count("/emission/helper_cap_omissions") > 0,
        "the helper budget was never crossed: the helper-cap shape is not covered"
    );
    assert!(count("/emission/dfs_depth_omissions") > 0);
    assert!(count("/emission/dfs_visit_omissions") > 0);
    assert_eq!(
        count("/emission/structured_declines"),
        count("/emission/irreducible")
            + count("/emission/unsupported_region")
            + count("/emission/repeat_budget")
            + count("/emission/structured_depth_budget")
            + count("/emission/coverage_mismatch"),
        "structured decline accounting is not derived from its causes"
    );
    assert_eq!(
        count("/emission/structured_rollbacks"),
        count("/emission/repeat_budget")
            + count("/emission/structured_depth_budget")
            + count("/emission/coverage_mismatch"),
        "rollback accounting is not derived from post-mutation causes"
    );
    assert_eq!(
        count("/emission/helper_cap_omissions"),
        count("/omitted_path_markers"),
        "helper-cap events and emitted omission markers differ"
    );
    assert_provenance_accounting(run);
    assert!(
        count("/block_helper_refs") > 0,
        "no block helper reference: helper order is not covered"
    );
    assert!(
        pseudocode.contains("inferred from: sub_"),
        "no program-level generic alias: the generic-alias shape is not covered"
    );
    assert!(
        sources.iter().any(|source| annotated_twin(source)),
        "no annotated twin of an identical line: the duplicate-line provenance \
         shape is not covered"
    );
    // Two alias candidates with an identical count: the declarations have to
    // come out in name order and not in the order the candidate map iterated.
    let first = pseudocode
        .find("final int reg8Minus1 =")
        .expect("no minus-one alias declaration: alias order is not covered");
    let second = pseudocode
        .find("final int reg9Minus1 =")
        .expect("only one minus-one alias: the tie the ordering rests on is absent");
    assert!(
        first < second,
        "the tied alias candidates were declared in map order, not in name order"
    );
    // The pool tie is a structural ordering choice, not a shape: the target two
    // pool entries claim has to be emitted under the lower index's name.
    assert!(
        pseudocode.contains(fixture::POOL_TIE_WINNER),
        "the contested pool target was not emitted under the lowest claiming \
         index's name"
    );
    assert!(
        !pseudocode.contains(fixture::POOL_TIE_LOSER),
        "the contested pool target was emitted under the higher claiming index's \
         name, so map order decided it"
    );
}

fn helper_symbols(source: &str) -> (Vec<String>, Vec<String>) {
    let mut calls = Vec::new();
    let mut definitions = Vec::new();
    for line in source.lines().map(str::trim_start) {
        if let Some(rest) = line.strip_prefix("return _block_") {
            if let Some(id) = rest.strip_suffix("();") {
                calls.push(format!("_block_{id}"));
            }
        }
        if let Some(rest) = line.strip_prefix("dynamic _block_") {
            if let Some(id) = rest.strip_suffix("() {") {
                definitions.push(format!("_block_{id}"));
            }
        }
    }
    (calls, definitions)
}

fn assert_provenance_accounting(run: &Run) {
    let bytes = run
        .artifacts
        .iter()
        .find(|(name, _)| name == "provenance.jsonl")
        .map(|(_, bytes)| bytes)
        .expect("provenance audit artifact is missing");
    let mut actual: std::collections::BTreeMap<(u64, String), (u64, u64)> =
        std::collections::BTreeMap::new();
    let mut accounting: std::collections::BTreeMap<(u64, String), (u64, u64, u64, i64)> =
        std::collections::BTreeMap::new();
    let mut filter_rejections = 0u64;
    let mut saw_filter_plant = false;

    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let row: serde_json::Value = serde_json::from_slice(line).expect("provenance row parses");
        let record = row["record"]
            .as_str()
            .expect("provenance row has a record kind");
        let Some(loss_site) = row["loss_site"].as_str() else {
            continue;
        };
        let function_id = row["function_id"]
            .as_u64()
            .expect("provenance row has a function id");
        let key = (function_id, loss_site.to_string());
        match record {
            "annotation" => actual.entry(key.clone()).or_default().0 += 1,
            "cap_omission" => actual.entry(key.clone()).or_default().1 += 1,
            "filter_rejection" => {
                actual.entry(key.clone()).or_default().1 += 1;
                filter_rejections += 1;
                saw_filter_plant |= function_id == 0
                    && row["reason"] == "names_absent_identifier"
                    && row["rendered"] == "0x11";
            }
            _ => {}
        }
        if matches!(record, "annotation" | "cap_summary") {
            let summary = (
                row["candidates_considered"]
                    .as_u64()
                    .expect("considered count"),
                row["accepted"].as_u64().expect("accepted count"),
                row["rejected"].as_u64().expect("rejected count"),
                row["unaccounted_candidates"]
                    .as_i64()
                    .expect("unaccounted count"),
            );
            if let Some(previous) = accounting.insert(key, summary) {
                assert_eq!(
                    previous, summary,
                    "one provenance stream disagrees with itself"
                );
            }
        }
    }

    let mut accepted = 0u64;
    let mut rejected = 0u64;
    for (key, (considered, expected_accepted, expected_rejected, unaccounted)) in accounting {
        let observed = actual.get(&key).copied().unwrap_or_default();
        assert_eq!(unaccounted, 0, "{key:?}: unaccounted provenance candidate");
        assert_eq!(considered, expected_accepted + expected_rejected, "{key:?}");
        assert_eq!(observed, (expected_accepted, expected_rejected), "{key:?}");
        accepted += expected_accepted;
        rejected += expected_rejected;
    }
    assert!(accepted > 0, "the provenance fixture accepted no candidate");
    assert!(rejected > 0, "the provenance fixture rejected no candidate");
    assert!(
        filter_rejections > 0,
        "the provenance fixture reached no filter rejection"
    );
    assert!(
        saw_filter_plant,
        "the named filter-rejection provenance plant was not recorded"
    );
}

/// Does one function render the same line twice, with an annotation on exactly
/// one of them?
///
/// That is the shape an anchor can get wrong: strip the annotations and the two
/// lines are indistinguishable, so an annotation attached by text rather than by
/// line identity can land on the wrong twin.
fn annotated_twin(source: &str) -> bool {
    let mut bare: Vec<String> = Vec::new();
    let mut annotated: Vec<String> = Vec::new();
    for line in source.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let stripped = flutterdec_decompiler::strip_join_annotation_span(line)
            .trim()
            .to_string();
        if stripped == line {
            bare.push(stripped);
        } else {
            annotated.push(stripped);
        }
    }
    annotated.iter().any(|line| bare.contains(line))
}

/// Compare two `report.json` documents outside their frozen volatile scalars.
///
/// Returns the located spans of the left document, so the caller can assert the
/// list is neither empty nor drifting.
fn compare_reports(index: usize, left: &[u8], right: &[u8]) -> Vec<raw_json::Located> {
    let left_spans = raw_json::locate(left, &FROZEN_POINTERS).expect("run 0 report.json scans");
    let right_spans =
        raw_json::locate(right, &FROZEN_POINTERS).unwrap_or_else(|e| panic!("run {index}: {e}"));

    let left_pointers: Vec<&str> = left_spans.iter().map(|s| s.pointer.as_str()).collect();
    let right_pointers: Vec<&str> = right_spans.iter().map(|s| s.pointer.as_str()).collect();
    assert_eq!(
        left_pointers, right_pointers,
        "run {index} located a different set of volatile pointers, so the frozen \
         allowlist does not describe both documents"
    );

    for (left_span, right_span) in left_spans.iter().zip(&right_spans) {
        assert_eq!(
            left_span.kind, right_span.kind,
            "run {index}: {} changed scalar type",
            left_span.pointer
        );
        // A path that is absent is `null` in both documents and is not volatile:
        // it stays inside the compared bytes rather than being excused.
        assert!(
            matches!(
                left_span.kind,
                raw_json::Scalar::String | raw_json::Scalar::Number | raw_json::Scalar::Null
            ),
            "run {index}: {} is neither a string, a number nor null",
            left_span.pointer
        );
    }

    let frozen = |spans: &[raw_json::Located]| -> Vec<raw_json::Located> {
        spans
            .iter()
            .filter(|s| matches!(s.kind, raw_json::Scalar::String | raw_json::Scalar::Number))
            .cloned()
            .collect()
    };
    let left_frozen = frozen(&left_spans);
    let right_frozen = frozen(&right_spans);

    let left_slices = raw_json::untouched(left, &left_frozen);
    let right_slices = raw_json::untouched(right, &right_frozen);
    assert_eq!(
        left_slices.len(),
        right_slices.len(),
        "run {index} has a different number of untouched slices"
    );
    for (slice_index, (want, got)) in left_slices.iter().zip(&right_slices).enumerate() {
        if want == got {
            continue;
        }
        let at = want
            .iter()
            .zip(got.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(want.len().min(got.len()));
        let window = |bytes: &[u8]| {
            String::from_utf8_lossy(&bytes[at.saturating_sub(80)..(at + 80).min(bytes.len())])
                .to_string()
        };
        panic!(
            "run {index}: report.json differs outside every frozen volatile scalar, \
             in untouched slice {slice_index} at offset {at}\n  run 0:  {}\n  run {index}: {}",
            window(want),
            window(got)
        );
    }
    left_frozen
}

// ---------------------------------------------------------------------------
// The check
// ---------------------------------------------------------------------------

#[test]
fn the_whole_artifact_set_is_byte_identical_in_twenty_processes() {
    if let Some(dir) = std::env::var_os(CHILD_REQUEST) {
        let root = PathBuf::from(dir);
        let libapp = plant_workspace(&root);
        let outcome = run_decompile(&root, &libapp, &options(root.join("out")));
        // The harness leaves `test <name> ... ` unterminated, so the first line
        // printed here would otherwise carry that prefix and not match.
        println!();
        println!("{CANARY_PREFIX}{}", hash_order_canary());
        // Not the message: it names absolute paths. The gate verdict itself is
        // in `quality.json` and is compared byte for byte with everything else.
        println!(
            "{OUTCOME_PREFIX}{}",
            if outcome.is_ok() { "ok" } else { "gate-failed" }
        );
        return;
    }

    let keep = std::env::var_os("FLUTTERDEC_PIPELINE_DETERMINISM_KEEP").map(PathBuf::from);
    let scratch = keep.clone().map(Ok).unwrap_or_else(|| {
        tempfile::tempdir()
            .map(|d| d.keep())
            .map_err(|e| e.to_string())
    });
    let scratch = scratch.expect("scratch directory");
    fs::create_dir_all(&scratch).expect("create scratch");

    let exe = std::env::current_exe().expect("the running test binary");
    let mut canaries: BTreeSet<String> = BTreeSet::new();
    let mut outcomes: BTreeSet<String> = BTreeSet::new();
    let mut roots = Vec::with_capacity(PROCESSES);

    for index in 0..PROCESSES {
        // Deliberately uneven name lengths: `p9` and `p10` differ in width, so
        // the volatile path scalars differ in length and not only in content,
        // which is what the raw-slice comparison has to tolerate.
        let root = scratch.join(format!("p{index}"));
        fs::create_dir_all(&root).expect("create run root");
        let child = std::process::Command::new(&exe)
            .args(["--exact", "--nocapture", "--test-threads=1", CHILD_TEST])
            .env(CHILD_REQUEST, &root)
            .env("FLUTTERDEC_PROV_AUDIT", root.join("out/provenance.jsonl"))
            .env("FLUTTERDEC_PROV_SAMPLE", "pipeline-determinism")
            .env_remove("FLUTTERDEC_PIPELINE_DETERMINISM_KEEP")
            .output()
            .expect("re-execute the test binary");
        assert!(
            child.status.success(),
            "process {index} failed: {}",
            String::from_utf8_lossy(&child.stderr)
        );
        let stdout = String::from_utf8(child.stdout).expect("child output is utf-8");
        let mut saw_canary = false;
        for line in stdout.lines() {
            if let Some(value) = line.strip_prefix(CANARY_PREFIX) {
                canaries.insert(value.to_string());
                saw_canary = true;
            }
            if let Some(value) = line.strip_prefix(OUTCOME_PREFIX) {
                outcomes.insert(value.to_string());
            }
        }
        assert!(
            saw_canary,
            "process {index} printed no canary, so `{CHILD_TEST}` named no test"
        );
        roots.push(root);
    }

    assert!(
        canaries.len() > 1,
        "all {PROCESSES} processes iterated a `HashSet` in the same order, so this \
         comparison proves nothing about the pipeline"
    );
    assert_eq!(
        outcomes.len(),
        1,
        "the pipelines disagreed about the quality gate: {outcomes:?}"
    );

    let runs: Vec<Run> = roots.iter().map(|r| read_run(&r.join("out"))).collect();
    for (index, run) in runs.iter().enumerate() {
        assert_fixture_covers_every_shape(run);
        assert!(
            !run.artifacts.is_empty(),
            "process {index} produced no artifacts before byte comparison"
        );
    }

    let first = &runs[0];
    assert!(
        first.artifacts.len() >= fixture::FUNCTIONS.len() * 2,
        "expected pseudocode and emitted IR for every function, got {} artifacts",
        first.artifacts.len()
    );

    for (index, run) in runs.iter().enumerate().skip(1) {
        let names = |r: &Run| {
            r.artifacts
                .iter()
                .map(|(n, _)| n.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(first),
            names(run),
            "run {index} emitted a different artifact set, so program or function \
             order drifted"
        );
        for ((name, want), (_, got)) in first.artifacts.iter().zip(&run.artifacts) {
            assert_eq!(
                want,
                got,
                "run {index}: {name} differs\n  run 0:  {}\n  run {index}: {}",
                String::from_utf8_lossy(want),
                String::from_utf8_lossy(got)
            );
        }
        assert_eq!(
            String::from_utf8_lossy(&first.quality),
            String::from_utf8_lossy(&run.quality),
            "run {index}: quality.json is not byte identical"
        );
        let located = compare_reports(index, &first.report, &run.report);
        for required in REQUIRED_VOLATILE {
            assert!(
                located.iter().any(|s| s.pointer == required),
                "run {index}: {required} was not located as a volatile scalar, so the \
                 comparison tolerated nothing and proves less than it claims"
            );
        }
    }

    if keep.is_none() {
        let _ = fs::remove_dir_all(&scratch);
    }
}

// ---------------------------------------------------------------------------
// The comparison machinery's own tests
// ---------------------------------------------------------------------------

/// A difference inside a frozen scalar is tolerated; the same difference one
/// byte outside it is not.
#[test]
fn the_raw_comparison_tolerates_only_the_frozen_scalars() {
    let left = br#"{"input":"/a/x","counts":{"functions":3},"ghidra_script":{"path":"/a/x/g.py"}}"#;
    let right =
        br#"{"input":"/bb/y","counts":{"functions":3},"ghidra_script":{"path":"/bb/y/g.py"}}"#;
    let drifted =
        br#"{"input":"/a/x","counts":{"functions":4},"ghidra_script":{"path":"/a/x/g.py"}}"#;

    compare_reports(1, left, right);

    let panicked = std::panic::catch_unwind(|| compare_reports(1, left, drifted)).is_err();
    assert!(
        panicked,
        "a changed count outside every frozen scalar must fail the comparison"
    );
}

/// The locator reports the scalar's own byte range and its type, and refuses a
/// pointer that does not exist rather than inventing one.
#[test]
fn the_locator_reports_exact_scalar_ranges() {
    let bytes = br#"{"input": "/tmp/a", "n": -1.5e3, "engine_symbol_ingestion": {"manifest_path": null, "loaded_paths": ["/p/one", "/p/two"]}}"#;
    let found = raw_json::locate(
        bytes,
        &[
            "/input",
            "/n",
            "/engine_symbol_ingestion/manifest_path",
            "/engine_symbol_ingestion/loaded_paths/*",
            "/absent",
        ],
    )
    .expect("scans");

    let rendered: Vec<(String, String, raw_json::Scalar)> = found
        .iter()
        .map(|s| {
            (
                s.pointer.clone(),
                String::from_utf8_lossy(&bytes[s.start..s.end]).to_string(),
                s.kind,
            )
        })
        .collect();
    assert_eq!(
        rendered,
        vec![
            (
                "/input".to_string(),
                "\"/tmp/a\"".to_string(),
                raw_json::Scalar::String
            ),
            (
                "/n".to_string(),
                "-1.5e3".to_string(),
                raw_json::Scalar::Number
            ),
            (
                "/engine_symbol_ingestion/manifest_path".to_string(),
                "null".to_string(),
                raw_json::Scalar::Null
            ),
            (
                "/engine_symbol_ingestion/loaded_paths/0".to_string(),
                "\"/p/one\"".to_string(),
                raw_json::Scalar::String
            ),
            (
                "/engine_symbol_ingestion/loaded_paths/1".to_string(),
                "\"/p/two\"".to_string(),
                raw_json::Scalar::String
            ),
        ]
    );
}

/// The untouched slices really do cover everything outside the frozen ranges:
/// concatenating them and the located scalars rebuilds the document.
#[test]
fn the_untouched_slices_and_the_frozen_scalars_partition_the_document() {
    let bytes = br#"{"a": "x", "b": [1, 2], "c": {"d": "y"}}"#;
    let spans = raw_json::locate(bytes, &["/a", "/c/d"]).expect("scans");
    let slices = raw_json::untouched(bytes, &spans);

    let mut rebuilt = Vec::new();
    for (index, slice) in slices.iter().enumerate() {
        rebuilt.extend_from_slice(slice);
        if let Some(span) = spans.get(index) {
            rebuilt.extend_from_slice(&bytes[span.start..span.end]);
        }
    }
    assert_eq!(rebuilt, bytes.to_vec());
}
