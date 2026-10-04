/ fcn.14603da90(int64_t arg1);
|           ; arg int64_t arg1 @ rcx
|           ; var int64_t var_18h @ stack - 0x18
|           ; var int64_t var_8h @ stack + 0x8
|           ; var int64_t var_10h @ stack + 0x10
|           0x14603da90      mov   qword [var_10h], rbx
|           0x14603da95      push  rdi
|           0x14603da96      sub   rsp, 0x30
|           0x14603da9a      mov   rdi, rcx                            ; arg1
|           0x14603da9d      lea   rbx, qword [rcx+0x50]               ; arg1
|           0x14603daa1      mov   qword [var_8h], rbx
|           0x14603daa6      mov   rcx, rbx
|           0x14603daa9      call  fcn.1427c8210
|           0x14603daae      nop
|           0x14603daaf      mov   rcx, qword [rdi+0x58]
|           0x14603dab3      mov   rax, qword [rcx]
|           0x14603dab6      mov   byte [var_18h], 0x00
|           0x14603dabb      mov   r9d, 0x02
|           0x14603dac1      xor   r8d, r8d
|           0x14603dac4      lea   edx, qword [r9-0x01]
|           0x14603dac8      call  qword [rax+0x70]                    ; 112
|           0x14603dacb      movzx edi, al
|           0x14603dace      mov   rcx, rbx
|           0x14603dad1      call  fcn.1427c8240
|           0x14603dad6      nop
|           0x14603dad7      movzx eax, dil
|           0x14603dadb      mov   rbx, qword [var_10h]
|           0x14603dae0      add   rsp, 0x30
|           0x14603dae4      pop   rdi
\           0x14603dae5      ret
