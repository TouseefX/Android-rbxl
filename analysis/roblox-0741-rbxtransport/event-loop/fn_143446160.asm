/ fcn.143446160(int64_t arg1, int64_t arg2);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; var int64_t var_98h @ stack - 0x98
|           ; var int64_t var_90h @ stack - 0x90
|           ; var int64_t var_88h @ stack - 0x88
|           ; var int64_t var_18h @ stack - 0x18
|           0x143446160      sub   rsp, 0xb8
|           0x143446167      mov   dword [var_90h], edx                ; arg2
|           0x14344616b      lea   rax, qword [0x1489f9900]            ; "0RDC\U00000001"
|           0x143446172      mov   qword [var_98h], rcx                ; arg1
|           0x143446177      lea   rdx, qword [var_88h]
|           0x14344617c      movups xmm0, xmmword [var_98h]
|           0x143446181      and   rax, 0xfffffffffffffffb
|           0x143446185      or    rax, 0x02
|           0x143446189      mov   qword [var_18h], rax
|           0x143446191      movaps xmmword [var_88h], xmm0
|           0x143446196      call  fcn.143445f60
|           0x14344619b      add   rsp, 0xb8
\           0x1434461a2      ret
