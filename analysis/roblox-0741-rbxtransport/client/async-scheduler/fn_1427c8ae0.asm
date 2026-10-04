/ fcn.1427c8ae0(int64_t arg1, int64_t arg3);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg3 @ r8
|           0x1427c8ae0      push  rbx
|           0x1427c8ae2      sub   rsp, 0x20
|           0x1427c8ae6      mov   rbx, rcx                            ; arg1
|           0x1427c8ae9      test  rcx, rcx                            ; arg1
|       ,=< 0x1427c8aec      jz    0x1427c8b07
|       |   0x1427c8aee      mov   qword [rcx], r8                     ; arg3
|       |   0x1427c8af1      add   rcx, 0x10                           ; 16 ; arg1
|       |   0x1427c8af5      call  r8
|       |   0x1427c8af8      mov   qword [rbx+0x08], rax
|       |   0x1427c8afc      mov   qword [rbx+0x90], 0x00
|       `-> 0x1427c8b07      mov   rax, rbx
|           0x1427c8b0a      add   rsp, 0x20
|           0x1427c8b0e      pop   rbx
\           0x1427c8b0f      ret
