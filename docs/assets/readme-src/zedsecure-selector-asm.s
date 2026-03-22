0x2e5960: mov x3, #2
0x2e5964: ldur x0, [x2, #-1]
0x2e5968: ubfx x0, x0, #0xc, #0x14
0x2e596c: mov x1, x2
0x2e5970: ldr x2, [x27, #0x6cf0] ; pool[27888]
0x2e5974: sub x30, x0, #0xffe
0x2e5978: ldr x30, [x21, x30, lsl #3]
0x2e597c: blr x30 ; indirect call
...
0x2e59f0: ldur x1, [x0, #-1]
0x2e59f4: ubfx x1, x1, #0xc, #0x14
0x2e5a04: ldr x2, [x27, #0x6cf0] ; pool[27888]
0x2e5a10: blr x30 ; indirect call
