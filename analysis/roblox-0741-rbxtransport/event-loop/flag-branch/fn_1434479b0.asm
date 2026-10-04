/ fcn.1434479b0(int64_t arg1);
|           ; arg int64_t arg1 @ rcx
|           ; var int64_t var_8h @ stack + 0x8
|           0x1434479b0      mov   qword [var_8h], rbx
|           0x1434479b5      push  rdi
|           0x1434479b6      sub   rsp, 0x20
|           0x1434479ba      mov   rbx, qword [rcx+0x40]               ; arg1
|           0x1434479be      mov   rdi, rcx                            ; arg1
|           0x1434479c1      sub   rbx, qword [rcx+0x38]               ; arg1
|           0x1434479c5      sar   rbx, 0x03
|           0x1434479c9      call  fcn.1438696e0
|           0x1434479ce      mov   eax, eax
|           0x1434479d0      xor   edx, edx
|           0x1434479d2      div   rbx
|           0x1434479d5      mov   rax, qword [rdi+0x38]
|           0x1434479d9      mov   rbx, qword [var_8h]
|           0x1434479de      mov   rax, qword [rax+rdx*8]
|           0x1434479e2      add   rsp, 0x20
|           0x1434479e6      pop   rdi
\           0x1434479e7      ret
