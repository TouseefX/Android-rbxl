/ fcn.14603e7b0(int64_t arg1);
|           ; arg int64_t arg1 @ rcx
|           ; var int64_t var_48h @ stack - 0x48
|           ; var int64_t var_40h @ stack - 0x40
|           ; var int64_t var_10h @ stack - 0x10
|           0x14603e7b0      sub   rsp, 0x68
|           0x14603e7b4      xor   eax, eax
|           0x14603e7b6      lea   rdx, qword [var_48h]
|           0x14603e7bb      xchg  byte [rcx+0x80], al                 ; arg1
|           0x14603e7c1      lea   rax, qword [0x148f73218]            ; " \xb8\U00000003F\U00000001"
|           0x14603e7c8      mov   qword [var_40h], rcx                ; arg1
|           0x14603e7cd      mov   qword [var_48h], rax
|           0x14603e7d2      lea   rax, qword [var_48h]
|           0x14603e7d7      mov   qword [var_10h], rax
|           0x14603e7dc      call  fcn.14603e7f0
|           0x14603e7e1      mov   al, 0x01
|           0x14603e7e3      add   rsp, 0x68
\           0x14603e7e7      ret
