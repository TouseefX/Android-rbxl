/ fcn.140001b80();
|           ; var int64_t var_8h @ stack - 0x8
|           0x140001b80      push  rbp
|           0x140001b81      sub   rsp, 0x20
|           0x140001b85      lea   rbp, qword [var_8h]
|           0x140001b8a      mov   eax, dword [0x14d9872d8]            ; [0x14d9872d8:4]=0
|           0x140001b90      mov   rcx, qword gs:[0x58]
|           0x140001b99      mov   rax, qword [rcx+rax*8]
|           0x140001b9d      mov   al, byte [rax+0x460]
|           0x140001ba3      and   al, 0x01
|           0x140001ba5      movzx eax, al
|           0x140001ba8      and   eax, 0x01
|           0x140001bab      cmp   eax, 0x00
|       ,=< 0x140001bae      jnz   0x140001bdb
|       |   0x140001bb0      mov   eax, dword [0x14d9872d8]            ; [0x14d9872d8:4]=0
|       |   0x140001bb6      mov   rcx, qword gs:[0x58]
|       |   0x140001bbf      mov   rax, qword [rcx+rax*8]
|       |   0x140001bc3      mov   byte [rax+0x460], 0x01
|       |   0x140001bca      call  fcn.140001c10
|       |   0x140001bcf      lea   rcx, qword [0x140001d30]
|       |   0x140001bd6      call  fcn.14730fd90
|       `-> 0x140001bdb      mov   eax, dword [0x14d9872d8]            ; [0x14d9872d8:4]=0
|           0x140001be1      mov   rcx, qword gs:[0x58]
|           0x140001bea      mov   rax, qword [rcx+rax*8]
|           0x140001bee      lea   rax, qword [rax+0x40]
|           0x140001bf5      add   rax, 0x410                          ; 1040
|           0x140001bfb      add   rsp, 0x20
|           0x140001bff      pop   rbp
\           0x140001c00      ret
