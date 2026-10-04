/ fcn.1427c8b10(int64_t arg1);
|           ; arg int64_t arg1 @ rcx
|           0x1427c8b10      sub   rsp, 0x28
|           0x1427c8b14      mov   rax, qword [rcx]                    ; arg1
|           0x1427c8b17      test  rax, rax
|       ,=< 0x1427c8b1a      jz    0x1427c8b26
|       |   0x1427c8b1c      mov   rcx, qword [rcx+0x08]               ; arg1
|       |   0x1427c8b20      mov   rdx, rcx                            ; arg1
|       |   0x1427c8b23      call  rax
|       |   0x1427c8b25      nop
|       `-> 0x1427c8b26      add   rsp, 0x28
\           0x1427c8b2a      ret
