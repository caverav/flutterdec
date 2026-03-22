0x460358: add x16, x3, x2, lsl #2
0x46035c: ldur w4, [x16, #0xf]
0x460360: add x4, x4, x28, lsl #32
0x460364: mov x0, x4
0x460368: ldur x2, [x29, #-8]
0x46036c: stur x4, [x29, #-0x28]
0x460370: mov x1, x22
0x460374: cmp w2, w22
0x460378: b.eq #0x460394
0x46037c: ldur w4, [x2, #0x1b]
0x460380: add x4, x4, x28, lsl #32
0x460384: ldr x8, [x27, #0x650] ; pool[1616]
0x460388: ldur x9, [x4, #7]
0x46038c: ldr x3, [x27, #0x658] ; pool[1624] = "minWidth"
0x460390: blr x9
