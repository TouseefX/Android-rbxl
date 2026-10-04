/ fcn.14603b3d0(int64_t arg1, int64_t arg2);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           0x14603b3d0      push  rbx
|           0x14603b3d2      sub   rsp, 0x20
|           0x14603b3d6      mov   rbx, rcx                            ; arg1
|           0x14603b3d9      cmp   rcx, rdx                            ; arg2
|       ,=< 0x14603b3dc      jnz   0x14603b40b
|       |   0x14603b3de      mov   rcx, qword [rbx+0x40]
|       |   0x14603b3e2      add   rbx, 0x08
|       |   0x14603b3e6      test  rcx, rcx
|      ,==< 0x14603b3e9      jz    0x14603b487
|      ||   0x14603b3ef      mov   rax, qword [rcx]
|      ||   0x14603b3f2      cmp   rcx, rbx
|      ||   0x14603b3f5      setnz dl
|      ||   0x14603b3f8      call  qword [rax+0x20]                    ; 32
|      ||   0x14603b3fb      xor   eax, eax
|      ||   0x14603b3fd      mov   qword [rbx+0x38], 0x00
|      ||   0x14603b405      add   rsp, 0x20
|      ||   0x14603b409      pop   rbx
|      ||   0x14603b40a      ret
|      |`-> 0x14603b40b      test  rdx, rdx                            ; arg2
|      |,=< 0x14603b40e      jz    0x14603b43a
|      ||   0x14603b410      test  rbx, rbx
|     ,===< 0x14603b413      jz    0x14603b431
|     |||   0x14603b415      mov   rax, qword [rdx]                    ; arg2
|     |||   0x14603b418      add   rdx, 0x08                           ; arg2
|     |||   0x14603b41c      mov   qword [rcx], rax                    ; arg1
|     |||   0x14603b41f      add   rcx, 0x08                           ; arg1
|     |||   0x14603b423      call  fcn.1407490a0
|     |||   0x14603b428      mov   rax, rbx
|     |||   0x14603b42b      add   rsp, 0x20
|     |||   0x14603b42f      pop   rbx
|     |||   0x14603b430      ret
|     `---> 0x14603b431      mov   rax, rbx
|      ||   0x14603b434      add   rsp, 0x20
|      ||   0x14603b438      pop   rbx
|      ||   0x14603b439      ret
|      |`-> 0x14603b43a      mov   rcx, qword [rcx+0x40]               ; arg1
|      |    0x14603b43e      test  rcx, rcx                            ; arg1
|      |,=< 0x14603b441      jz    0x14603b449
|      ||   0x14603b443      mov   rax, qword [rcx]                    ; arg1
|      ||   0x14603b446      call  qword [rax+0x10]                    ; 16
|      |`-> 0x14603b449      mov   rax, qword [rbx]
|      |    0x14603b44c      movzx eax, byte [rax+0x09]
|      |    0x14603b450      nop
|      |    0x14603b451      test  al, al
|      |,=< 0x14603b453      jz    0x14603b487
|      ||   0x14603b455      nop   word [rax+rax*1], ax
|     .---> 0x14603b460      mov   rax, qword [rbx]
|     :||   0x14603b463      movzx eax, byte [rax+0x20]
|     :||   0x14603b467      nop
|     :||   0x14603b468      test  al, al
|    ,====< 0x14603b46a      jz    0x14603b487
|    |:||   0x14603b46c      mov   rcx, qword [rbx]
|    |:||   0x14603b46f      call  fcn.14603daf0
|    |:||   0x14603b474      mov   cl, 0x01
|    |:||   0x14603b476      call  fcn.1427c81d0
|    |:||   0x14603b47b      mov   rax, qword [rbx]
|    |:||   0x14603b47e      movzx eax, byte [rax+0x09]
|    |:||   0x14603b482      nop
|    |:||   0x14603b483      test  al, al
|    |`===< 0x14603b485      jnz   0x14603b460
|    `-``-> 0x14603b487      xor   eax, eax
|           0x14603b489      add   rsp, 0x20
|           0x14603b48d      pop   rbx
\           0x14603b48e      ret
