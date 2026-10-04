/ fcn.143445620(int64_t arg1);
|           ; arg int64_t arg1 @ rcx
|           ; var int64_t var_70h @ stack - 0x70
|           ; var int64_t var_58h @ stack - 0x58
|           ; var int64_t var_50h @ stack - 0x50
|           ; var int64_t var_38h @ stack - 0x38
|           ; var int64_t var_30h @ stack - 0x30
|           ; var int64_t var_28h @ stack - 0x28
|           0x143445620      mov   r11, rsp
|           0x143445623      mov   qword [r11+0x10], rbx
|           0x143445627      mov   qword [r11+0x18], rbp
|           0x14344562b      mov   qword [r11+0x20], rsi
|           0x14344562f      push  rdi
|           0x143445630      push  r12
|           0x143445632      push  r13
|           0x143445634      push  r14
|           0x143445636      push  r15
|           0x143445638      sub   rsp, 0x70
|           0x14344563c      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x143445643      xor   rax, rsp
|           0x143445646      mov   qword [var_30h], rax
|           0x14344564b      mov   rdi, rcx                            ; arg1
|           0x14344564e      xor   bpl, bpl
|           0x143445651      mov   byte [rcx+0x588], 0x01              ; arg1
|           0x143445658      mov   qword [r11-0x70], rcx               ; arg1
|           0x14344565c      lea   rax, qword [0x1489f9908]            ; "`RDC\U00000001"
|           0x143445663      and   rax, 0xfffffffffffffffb
|           0x143445667      or    rax, 0x02
|           0x14344566b      mov   qword [r11-0x58], rax
|           0x14344566f      lea   rdx, qword [r11-0x70]
|           0x143445673      lea   rcx, qword [r11-0x50]
|           0x143445677      call  0x1407333c0
|           0x14344567c      nop
|           0x14344567d      mov   rbx, qword [var_58h]
|           0x143445682      mov   rax, rbx
|           0x143445685      and   rax, 0xfffffffffffffffc
|           0x143445689      mov   rcx, rax
|           0x14344568c      and   rcx, 0xfffffffffffffff8
|       ,=< 0x143445690      jz    0x1434456c2
|       |   0x143445692      sar   bl, 0x01
|       |   0x143445694      and   bl, 0x01
|       |   0x143445697      sar   al, 0x02
|       |   0x14344569a      not   al
|       |   0x14344569c      test  al, 0x01                            ; 1
|      ,==< 0x14344569e      jnz   0x1434456b3
|      ||   0x1434456a0      mov   rax, qword [rcx+0x10]
|      ||   0x1434456a4      lea   rcx, qword [var_70h]
|      ||   0x1434456a9      test  bl, bl
|      ||   0x1434456ab      cmovz rcx, qword [var_70h]
|      ||   0x1434456b1      call  rax
|      `--> 0x1434456b3      test  bl, bl
|      ,==< 0x1434456b5      jnz   0x1434456c2
|      ||   0x1434456b7      mov   rcx, qword [var_70h]
|      ||   0x1434456bc      call  0x14385ca60
|      ||   0x1434456c1      nop
|      ``-> 0x1434456c2      xor   r14d, r14d
|           0x1434456c5      mov   rbx, qword [rdi+0x2a8]
|           0x1434456cc      mov   rcx, qword [rdi+0x2b0]
|           0x1434456d3      sub   rcx, rbx
|           0x1434456d6      mov   r12, 0x6666666666666667             ; 'gfffffff'
|           0x1434456e0      mov   rax, r12
|           0x1434456e3      imul  rcx
|           0x1434456e6      sar   rdx, 0x06
|           0x1434456ea      mov   rax, rdx
|           0x1434456ed      shr   rax, 0x3f
|           0x1434456f1      add   rdx, rax
|       ,=< 0x1434456f4      jz    0x143445775
|       |   0x1434456f6      mov   esi, r14d
|       |   0x1434456f9      lea   r15d, qword [r14+0x01]
|       |   0x1434456fd      nop   dword [rax], eax
|      .--> 0x143445700      cmp   byte [rbx+rsi*1+0x90], 0x00
|     ,===< 0x143445708      jz    0x143445710
|     |:|   0x14344570a      movzx ebp, r15b
|    ,====< 0x14344570e      jmp   0x143445741
|    |`---> 0x143445710      lea   rcx, qword [rsi+0x10]
|    | :|   0x143445714      add   rcx, rbx
|    | :|   0x143445717      mov   rdx, qword [rcx+0x70]
|    | :|   0x14344571b      movzx eax, dl
|    | :|   0x14344571e      sar   al, 0x01
|    | :|   0x143445720      test  r15b, al
|    |,===< 0x143445723      jnz   0x143445728
|    ||:|   0x143445725      mov   rcx, qword [rcx]
|    |`---> 0x143445728      and   rdx, 0xfffffffffffffff8
|    | :|   0x14344572c      mov   rax, qword [rdx]
|    | :|   0x14344572f      call  rax
|    | :|   0x143445731      movzx ebp, bpl
|    | :|   0x143445735      cmp   byte [rbx+rsi*1+0x90], 0x00
|    | :|   0x14344573d      cmovnz ebp, r15d
|    | :|   ; CODE XREF from fcn.143445620 @ 0x14344570e
|    `----> 0x143445741      inc   r14
|      :|   0x143445744      add   rsi, 0xa0                           ; 160
|      :|   0x14344574b      mov   rbx, qword [rdi+0x2a8]
|      :|   0x143445752      mov   rcx, qword [rdi+0x2b0]
|      :|   0x143445759      sub   rcx, rbx
|      :|   0x14344575c      mov   rax, r12
|      :|   0x14344575f      imul  rcx
|      :|   0x143445762      sar   rdx, 0x06
|      :|   0x143445766      mov   rax, rdx
|      :|   0x143445769      shr   rax, 0x3f
|      :|   0x14344576d      add   rdx, rax
|      :|   0x143445770      cmp   r14, rdx
|      `==< 0x143445773      jb    0x143445700
|       `-> 0x143445775      mov   rax, qword [rdi+0x570]
|           0x14344577c      cmp   qword [rdi+0x568], rax
|       ,=< 0x143445783      jz    0x143445823
|       |   0x143445789      mov   rbx, qword [rdi+0x568]
|       |   0x143445790      mov   r14, rax
|       |   0x143445793      cmp   rbx, rax
|      ,==< 0x143445796      jz    0x1434457fb
|      ||   0x143445798      nop   dword [rax+rax*1], eax
|     .---> 0x1434457a0      mov   rsi, qword [rdi+0x2b0]
|     :||   0x1434457a7      cmp   rsi, qword [rdi+0x2b8]
|    ,====< 0x1434457ae      jz    0x1434457dd
|    |:||   0x1434457b0      mov   rax, qword [rbx]
|    |:||   0x1434457b3      mov   qword [rsi], rax
|    |:||   0x1434457b6      lea   rcx, qword [rsi+0x10]
|    |:||   0x1434457ba      lea   rdx, qword [rbx+0x10]
|    |:||   0x1434457be      call  0x143445170
|    |:||   0x1434457c3      movzx eax, byte [rbx+0x90]
|    |:||   0x1434457ca      mov   byte [rsi+0x90], al
|    |:||   0x1434457d0      add   qword [rdi+0x2b0], 0xa0             ; [0xa0:8]=-1 ; 160
|   ,=====< 0x1434457db      jmp   0x1434457ef
|   |`----> 0x1434457dd      mov   r8, rbx
|   | :||   0x1434457e0      mov   rdx, rsi
|   | :||   0x1434457e3      lea   rcx, qword [rdi+0x2a0]
|   | :||   0x1434457ea      call  0x143445300
|   | :||   ; CODE XREF from fcn.143445620 @ 0x1434457db
|   `-----> 0x1434457ef      add   rbx, 0xa0                           ; 160
|     :||   0x1434457f6      cmp   rbx, r14
|     `===< 0x1434457f9      jnz   0x1434457a0
|      `--> 0x1434457fb      lea   r8, qword [rdi+0x560]
|       |   0x143445802      mov   rdx, qword [rdi+0x570]
|       |   0x143445809      mov   rcx, qword [rdi+0x568]
|       |   0x143445810      call  0x143445270
|       |   0x143445815      mov   rax, qword [rdi+0x568]
|       |   0x14344581c      mov   qword [rdi+0x570], rax
|       `-> 0x143445823      test  bpl, bpl
|       ,=< 0x143445826      jz    0x14344593d
|       |   0x14344582c      lea   r13, qword [rdi+0x2a0]
|       |   0x143445833      mov   r14, qword [rdi+0x2b0]
|       |   0x14344583a      mov   rbx, qword [rdi+0x2a8]
|       |   0x143445841      cmp   rbx, r14
|      ,==< 0x143445844      jz    0x143445913
|      ||   0x14344584a      nop   word [rax+rax*1], ax
|     .---> 0x143445850      cmp   byte [rbx+0x90], 0x00
|    ,====< 0x143445857      jnz   0x143445865
|    |:||   0x143445859      add   rbx, 0xa0                           ; 160
|    |:||   0x143445860      cmp   rbx, r14
|    |`===< 0x143445863      jnz   0x143445850
|    `----> 0x143445865      cmp   rbx, r14
|     ,===< 0x143445868      jz    0x143445913
|     |||   0x14344586e      lea   rsi, qword [rbx+0xa0]
|     |||   0x143445875      cmp   rsi, r14
|    ,====< 0x143445878      jz    0x143445913
|    ||||   0x14344587e      lea   rdi, qword [rbx+0x10]
|   .-----> 0x143445882      cmp   byte [rsi+0x90], 0x00
|  ,======< 0x143445889      jnz   0x143445903
|  |:||||   0x14344588b      mov   rax, qword [rsi]
|  |:||||   0x14344588e      mov   qword [rbx], rax
|  |:||||   0x143445891      lea   r15, qword [rsi+0x10]
|  |:||||   0x143445895      cmp   rdi, r15
| ,=======< 0x143445898      jz    0x1434458e8
| ||:||||   0x14344589a      mov   rbp, qword [rdi+0x70]
| ||:||||   0x14344589e      mov   rax, rbp
| ||:||||   0x1434458a1      and   rax, 0xfffffffffffffffc
| ||:||||   0x1434458a5      mov   rcx, rax
| ||:||||   0x1434458a8      and   rcx, 0xfffffffffffffff8
| ========< 0x1434458ac      jz    0x1434458dd
| ||:||||   0x1434458ae      sar   bpl, 0x01
| ||:||||   0x1434458b1      and   bpl, 0x01
| ||:||||   0x1434458b5      sar   al, 0x02
| ||:||||   0x1434458b8      not   al
| ||:||||   0x1434458ba      test  al, 0x01                            ; 1
| ========< 0x1434458bc      jnz   0x1434458cf
| ||:||||   0x1434458be      mov   rax, qword [rcx+0x10]
| ||:||||   0x1434458c2      test  bpl, bpl
| ||:||||   0x1434458c5      mov   rcx, rdi
| ========< 0x1434458c8      jnz   0x1434458cd
| ||:||||   0x1434458ca      mov   rcx, qword [rdi]
| --------> 0x1434458cd      call  rax
| --------> 0x1434458cf      test  bpl, bpl
| ========< 0x1434458d2      jnz   0x1434458dd
| ||:||||   0x1434458d4      mov   rcx, qword [rdi]
| ||:||||   0x1434458d7      call  0x14385ca60
| ||:||||   0x1434458dc      nop
| --------> 0x1434458dd      mov   rdx, r15
| ||:||||   0x1434458e0      mov   rcx, rdi
| ||:||||   0x1434458e3      call  0x143445170
| `-------> 0x1434458e8      movzx eax, byte [rsi+0x90]
|  |:||||   0x1434458ef      mov   byte [rdi+0x80], al
|  |:||||   0x1434458f5      add   rbx, 0xa0                           ; 160
|  |:||||   0x1434458fc      add   rdi, 0xa0                           ; 160
|  `------> 0x143445903      add   rsi, 0xa0                           ; 160
|   :||||   0x14344590a      cmp   rsi, r14
|   `=====< 0x14344590d      jnz   0x143445882
|    ```--> 0x143445913      cmp   rbx, r14
|      ,==< 0x143445916      jz    0x14344593d
|      ||   0x143445918      mov   r8, rbx
|      ||   0x14344591b      mov   rdx, qword [r13+0x10]
|      ||   0x14344591f      mov   rcx, r14
|      ||   0x143445922      call  0x143445490
|      ||   0x143445927      mov   rbx, rax
|      ||   0x14344592a      mov   r8, r13
|      ||   0x14344592d      mov   rdx, qword [r13+0x10]
|      ||   0x143445931      mov   rcx, rax
|      ||   0x143445934      call  0x143445270
|      ||   0x143445939      mov   qword [r13+0x10], rbx
|      ``-> 0x14344593d      mov   rbx, qword [var_38h]
|           0x143445942      mov   rax, rbx
|           0x143445945      and   rax, 0xfffffffffffffffc
|           0x143445949      test  rax, 0xfffffffffffffff8
|       ,=< 0x14344594f      jz    0x14344596f
|       |   0x143445951      sar   bl, 0x01
|       |   0x143445953      test  bl, 0x01                            ; 1
|       |   0x143445956      lea   rcx, qword [var_50h]
|       |   0x14344595b      cmovz rcx, qword [var_50h]
|       |   0x143445961      and   rax, 0xfffffffffffffff8
|       |   0x143445965      mov   rdx, qword [rax]
|       |   0x143445968      call  rdx
|       |   0x14344596a      mov   rbx, qword [var_38h]
|       `-> 0x14344596f      mov   rax, rbx
|           0x143445972      and   rax, 0xfffffffffffffffc
|           0x143445976      test  rax, 0xfffffffffffffff8
|       ,=< 0x14344597c      jz    0x1434459b6
|       |   0x14344597e      sar   bl, 0x01
|       |   0x143445980      and   bl, 0x01
|       |   0x143445983      movzx ecx, al
|       |   0x143445986      sar   cl, 0x02
|       |   0x143445989      not   cl
|       |   0x14344598b      test  cl, 0x01                            ; 1
|      ,==< 0x14344598e      jnz   0x1434459a7
|      ||   0x143445990      and   rax, 0xfffffffffffffff8
|      ||   0x143445994      mov   rdx, qword [rax+0x10]
|      ||   0x143445998      lea   rcx, qword [var_50h]
|      ||   0x14344599d      test  bl, bl
|      ||   0x14344599f      cmovz rcx, qword [var_50h]
|      ||   0x1434459a5      call  rdx
|      `--> 0x1434459a7      test  bl, bl
|      ,==< 0x1434459a9      jnz   0x1434459b6
|      ||   0x1434459ab      mov   rcx, qword [var_50h]
|      ||   0x1434459b0      call  0x14385ca60
|      ||   0x1434459b5      nop
|      ``-> 0x1434459b6      mov   rcx, qword [var_30h]
|           0x1434459bb      xor   rcx, rsp
|           0x1434459be      call  fcn.14730fca0
|           0x1434459c3      lea   r11, qword [var_28h]
|           0x1434459c8      mov   rbx, qword [r11+0x38]
|           0x1434459cc      mov   rbp, qword [r11+0x40]
|           0x1434459d0      mov   rsi, qword [r11+0x48]
|           0x1434459d4      mov   rsp, r11
|           0x1434459d7      pop   r15
|           0x1434459d9      pop   r14
|           0x1434459db      pop   r13
|           0x1434459dd      pop   r12
|           0x1434459df      pop   rdi
\           0x1434459e0      ret
