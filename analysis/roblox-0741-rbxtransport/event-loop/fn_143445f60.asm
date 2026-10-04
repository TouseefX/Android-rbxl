/ fcn.143445f60(int64_t arg1, int64_t arg2);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; var int64_t var_138h @ stack - 0x138
|           ; var int64_t var_128h @ stack - 0x128
|           ; var int64_t var_a8h @ stack - 0xa8
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_20h @ stack - 0x20
|           ; var int64_t var_10h @ stack + 0x10
|           ; var int64_t var_18h @ stack + 0x18
|           0x143445f60      mov   qword [var_18h], rbx
|           0x143445f65      mov   qword [var_10h], rdx                ; arg2
|           0x143445f6a      push  rsi
|           0x143445f6b      push  rdi
|           0x143445f6c      push  r14
|           0x143445f6e      sub   rsp, 0x140
|           0x143445f75      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x143445f7c      xor   rax, rsp
|           0x143445f7f      mov   qword [var_20h], rax
|           0x143445f87      mov   rsi, rdx                            ; arg2
|           0x143445f8a      mov   rdi, rcx                            ; arg1
|           0x143445f8d      mov   qword [var_28h], rdx                ; arg2
|           0x143445f95      mov   ebx, dword [rcx+0x18]               ; arg1
|           0x143445f98      nop
|           0x143445f99      call  0x1473107b8
|           0x143445f9e      mov   rdx, rsi
|           0x143445fa1      cmp   eax, ebx
|       ,=< 0x143445fa3      jnz   0x143445fc3
|       |   0x143445fa5      mov   rcx, qword [rsi+0x70]
|       |   0x143445fa9      movzx eax, cl
|       |   0x143445fac      sar   al, 0x01
|       |   0x143445fae      test  al, 0x01                            ; 1
|      ,==< 0x143445fb0      jnz   0x143445fb5
|      ||   0x143445fb2      mov   rdx, qword [rsi]
|      `--> 0x143445fb5      and   rcx, 0xfffffffffffffff8
|       |   0x143445fb9      mov   rax, qword [rcx]
|       |   0x143445fbc      mov   rcx, rdx
|       |   0x143445fbf      call  rax
|      ,==< 0x143445fc1      jmp   0x143446038
|      |`-> 0x143445fc3      lea   rcx, qword [var_128h]
|      |    0x143445fc8      call  0x143445170
|      |    0x143445fcd      mov   r14, rax
|      |    0x143445fd0      mov   qword [var_138h], rax
|      |    0x143445fd5      mov   rdi, qword [rdi+0x08]
|      |    0x143445fd9      mov   rdx, qword [rdi]
|      |    0x143445fdc      mov   rbx, qword [rdx+0x10]
|      |    0x143445fe0      mov   rdx, rax
|      |    0x143445fe3      lea   rcx, qword [var_a8h]
|      |    0x143445feb      call  0x143445170
|      |    0x143445ff0      mov   rdx, rax
|      |    0x143445ff3      mov   rcx, rdi
|      |    0x143445ff6      call  rbx
|      |    0x143445ff8      nop
|      |    0x143445ff9      mov   rbx, qword [r14+0x70]
|      |    0x143445ffd      mov   rax, rbx
|      |    0x143446000      and   rax, 0xfffffffffffffffc
|      |    0x143446004      mov   rcx, rax
|      |    0x143446007      and   rcx, 0xfffffffffffffff8
|      |,=< 0x14344600b      jz    0x143446038
|      ||   0x14344600d      sar   bl, 0x01
|      ||   0x14344600f      and   bl, 0x01
|      ||   0x143446012      sar   al, 0x02
|      ||   0x143446015      not   al
|      ||   0x143446017      test  al, 0x01                            ; 1
|     ,===< 0x143446019      jnz   0x14344602b
|     |||   0x14344601b      mov   rax, qword [rcx+0x10]
|     |||   0x14344601f      test  bl, bl
|     |||   0x143446021      mov   rcx, r14
|    ,====< 0x143446024      jnz   0x143446029
|    ||||   0x143446026      mov   rcx, qword [r14]
|    `----> 0x143446029      call  rax
|     `---> 0x14344602b      test  bl, bl
|     ,===< 0x14344602d      jnz   0x143446038
|     |||   0x14344602f      mov   rcx, qword [r14]
|     |||   0x143446032      call  0x14385ca60
|     |||   0x143446037      nop
|     |||   ; CODE XREF from fcn.143445f60 @ 0x143445fc1
|     ```-> 0x143446038      mov   rcx, qword [rsi+0x70]
|           0x14344603c      test  rcx, 0xfffffffffffffff8
|       ,=< 0x143446043      jz    0x14344607e
|       |   0x143446045      movzx ebx, cl
|       |   0x143446048      sar   bl, 0x01
|       |   0x14344604a      and   bl, 0x01
|       |   0x14344604d      mov   rax, rcx
|       |   0x143446050      and   rax, 0xfffffffffffffffc
|       |   0x143446054      sar   al, 0x02
|       |   0x143446057      not   al
|       |   0x143446059      test  al, 0x01                            ; 1
|      ,==< 0x14344605b      jnz   0x143446071
|      ||   0x14344605d      and   rcx, 0xfffffffffffffff8
|      ||   0x143446061      mov   rax, qword [rcx+0x10]
|      ||   0x143446065      test  bl, bl
|      ||   0x143446067      mov   rcx, rsi
|     ,===< 0x14344606a      jnz   0x14344606f
|     |||   0x14344606c      mov   rcx, qword [rsi]
|     `---> 0x14344606f      call  rax
|      `--> 0x143446071      test  bl, bl
|      ,==< 0x143446073      jnz   0x14344607e
|      ||   0x143446075      mov   rcx, qword [rsi]
|      ||   0x143446078      call  0x14385ca60
|      ||   0x14344607d      nop
|      ``-> 0x14344607e      mov   rcx, qword [var_20h]
|           0x143446086      xor   rcx, rsp
|           0x143446089      call  fcn.14730fca0
|           0x14344608e      mov   rbx, qword [var_18h]
|           0x143446096      add   rsp, 0x140
|           0x14344609d      pop   r14
|           0x14344609f      pop   rdi
|           0x1434460a0      pop   rsi
\           0x1434460a1      ret
