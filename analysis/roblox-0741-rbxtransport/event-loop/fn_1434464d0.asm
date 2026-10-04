/ fcn.1434464d0(int64_t arg1);
|           ; arg int64_t arg1 @ rcx
|           0x1434464d0      push  rbx
|           0x1434464d2      sub   rsp, 0x20
|           0x1434464d6      movzx eax, byte [rcx+0x14]                ; arg1
|           0x1434464da      mov   rbx, rcx                            ; arg1
|           0x1434464dd      nop
|           0x1434464de      test  al, al
|       ,=< 0x1434464e0      jnz   0x143446517
|      .--> 0x1434464e2      call  0x1473107b8
|      :|   0x1434464e7      xchg  dword [rbx+0x18], eax
|      :|   0x1434464ea      mov   rcx, qword [rbx+0x08]
|      :|   0x1434464ee      mov   rax, qword [rcx]
|      :|   0x1434464f1      call  qword [rax+0x08]                    ; 8
|      :|   0x1434464f4      movzx eax, byte [rbx+0x14]
|      :|   0x1434464f8      nop
|      :|   0x1434464f9      test  al, al
|     ,===< 0x1434464fb      jnz   0x14344650e
|     |:|   0x1434464fd      cmp   byte [rbx+0x5b0], al
|    ,====< 0x143446503      jz    0x14344650e
|    ||:|   0x143446505      lea   rcx, qword [rbx+0x20]
|    ||:|   0x143446509      call  0x143445620
|    ``---> 0x14344650e      movzx eax, byte [rbx+0x14]
|      :|   0x143446512      nop
|      :|   0x143446513      test  al, al
|      `==< 0x143446515      jz    0x1434464e2
|       `-> 0x143446517      mov   rcx, qword [rbx+0x08]
|           0x14344651b      mov   rax, qword [rcx]
|           0x14344651e      call  qword [rax+0x28]                    ; 40
|           0x143446521      test  al, al
|       ,=< 0x143446523      jnz   0x14344655f
|      .--> 0x143446525      call  0x1473107b8
|      :|   0x14344652a      xchg  dword [rbx+0x18], eax
|      :|   0x14344652d      mov   rcx, qword [rbx+0x08]
|      :|   0x143446531      mov   rax, qword [rcx]
|      :|   0x143446534      call  qword [rax+0x08]                    ; 8
|      :|   0x143446537      movzx eax, byte [rbx+0x14]
|      :|   0x14344653b      nop
|      :|   0x14344653c      test  al, al
|     ,===< 0x14344653e      jnz   0x143446551
|     |:|   0x143446540      cmp   byte [rbx+0x5b0], al
|    ,====< 0x143446546      jz    0x143446551
|    ||:|   0x143446548      lea   rcx, qword [rbx+0x20]
|    ||:|   0x14344654c      call  0x143445620
|    ``---> 0x143446551      mov   rcx, qword [rbx+0x08]
|      :|   0x143446555      mov   rax, qword [rcx]
|      :|   0x143446558      call  qword [rax+0x28]                    ; 40
|      :|   0x14344655b      test  al, al
|      `==< 0x14344655d      jz    0x143446525
|       `-> 0x14344655f      mov   eax, dword [rbx+0x10]
|           0x143446562      nop
|           0x143446563      add   rsp, 0x20
|           0x143446567      pop   rbx
\           0x143446568      ret
