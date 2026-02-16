# IR Design (v1)

## LLIR

Instruction set:
- `LoadConst`
- `LoadMem`
- `StoreMem`
- `Call`
- `Branch`
- `Jump`
- `Return`
- `Other`

Each IR instruction preserves source VA for traceability.

## CFG

`FunctionIR` stores:
- function metadata (`FunctionInfo`)
- list of `BasicBlock`
- predecessor/successor indexes

Block boundaries are formed at:
- function entry
- branch targets
- fallthrough after conditional branches

## Determinism

Serialized IR output is stable by function order and block discovery order.
