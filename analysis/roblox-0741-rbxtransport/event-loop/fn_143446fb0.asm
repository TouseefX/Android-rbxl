/ fcn.143446fb0(int64_t arg1, int64_t arg2);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; var int64_t var_d8h @ stack - 0xd8
|           ; var int64_t var_c8h @ stack - 0xc8
|           ; var int64_t var_c0h @ stack - 0xc0
|           ; var int64_t var_b8h @ stack - 0xb8
|           ; var int64_t var_a8h @ stack - 0xa8
|           ; var int64_t var_a0h @ stack - 0xa0
|           ; var int64_t var_98h @ stack - 0x98
|           ; var int64_t var_88h @ stack - 0x88
|           ; var int64_t var_78h @ stack - 0x78
|           ; var int64_t var_68h @ stack - 0x68
|           ; var int64_t var_50h @ stack - 0x50
|           ; var int64_t var_48h @ stack - 0x48
|           ; var int64_t var_18h @ stack + 0x18
|           0x143446fb0      mov   qword [var_18h], rbx
|           0x143446fb5      push  rbp
|           0x143446fb6      push  rsi
|           0x143446fb7      push  rdi
|           0x143446fb8      push  r12
|           0x143446fba      push  r13
|           0x143446fbc      push  r14
|           0x143446fbe      push  r15
|           0x143446fc0      lea   rbp, qword [var_68h + 0x9]
|           0x143446fc5      sub   rsp, 0xc0
|           0x143446fcc      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x143446fd3      xor   rax, rsp
|           0x143446fd6      mov   qword [var_48h], rax
|           0x143446fda      mov   r12, rdx                            ; arg2
|           0x143446fdd      mov   r15, rcx                            ; arg1
|           0x143446fe0      mov   qword [var_98h], rcx                ; arg1
|           0x143446fe4      xor   edi, edi
|           0x143446fe6      mov   esi, edi
|           0x143446fe8      mov   dword [var_78h], edi
|           0x143446feb      mov   eax, dword [rdx]                    ; arg2
|           0x143446fed      mov   dword [rcx], eax                    ; arg1
|           0x143446fef      mov   eax, dword [rdx+0x04]               ; arg2
|           0x143446ff2      mov   dword [rcx+0x04], eax               ; arg1
|           0x143446ff5      add   rcx, 0x08                           ; arg1
|           0x143446ff9      lea   r14, qword [rdx+0x08]               ; arg2
|           0x143446ffd      mov   rdx, r14
|           0x143447000      call  0x140739200
|           0x143447005      nop
|           0x143447006      mov   qword [r15+0x28], rdi
|           0x14344700a      call  0x1427c7e70
|           0x14344700f      mov   qword [r15+0x30], rax
|           0x143447013      mov   qword [r15+0x38], rdi
|           0x143447017      mov   qword [r15+0x40], rdi
|           0x14344701b      mov   qword [r15+0x48], rdi
|           0x14344701f      lea   r13, qword [r15+0x50]
|           0x143447023      mov   qword [r13], rdi
|           0x143447027      call  0x1427c7e70
|           0x14344702c      mov   qword [r15+0x58], rax
|           0x143447030      mov   qword [r15+0x60], rdi
|           0x143447034      mov   qword [r15+0x68], rdi
|           0x143447038      mov   qword [r15+0x70], rdi
|           0x14344703c      mov   ebx, edi
|           0x14344703e      mov   qword [var_b8h], rbx
|           0x143447042      mov   r9, qword [r14+0x10]
|           0x143447046      mov   rdx, r14
|           0x143447049      mov   r10, qword [r14+0x18]
|           0x14344704d      cmp   r10, 0x10                           ; 16
|       ,=< 0x143447051      jb    0x143447056
|       |   0x143447053      mov   rdx, qword [r14]
|       `-> 0x143447056      cmp   r9, 0x05                            ; 5
|       ,=< 0x14344705a      jnz   0x1434470c5
|       |   0x14344705c      mov   rcx, rdi
|       |   0x14344705f      mov   r8, qword [0x1489f9b30]             ; [0x1489f9b30:8]=0x1489f9b40 "libuv" ; "@\x9b\x9fH\U00000001"
|       |   0x143447066      nop   word [rax+rax*1], ax
|      .--> 0x143447070      movzx eax, byte [rdx+rcx*1]
|      :|   0x143447074      inc   rcx
|      :|   0x143447077      cmp   al, byte [r8+rcx*1-0x01]
|     ,===< 0x14344707c      jnz   0x143447088
|     |:|   0x14344707e      cmp   rcx, 0x05                           ; 5
|     |`==< 0x143447082      jnz   0x143447070
|     | |   0x143447084      mov   eax, edi
|     |,==< 0x143447086      jmp   0x14344708d
|     `---> 0x143447088      sbb   eax, eax
|      ||   0x14344708a      or    eax, 0x01
|      ||   ; CODE XREF from fcn.143446fb0 @ 0x143447086
|      `--> 0x14344708d      test  eax, eax
|      ,==< 0x14344708f      jnz   0x1434470c5
|      ||   0x143447091      lea   ecx, qword [rax+0x38]
|      ||   0x143447094      call  0x14385bce0
|      ||   0x143447099      mov   qword [var_a8h], rax
|      ||   0x14344709d      test  rax, rax
|     ,===< 0x1434470a0      jz    0x1434470af
|     |||   0x1434470a2      mov   rcx, rax
|     |||   0x1434470a5      call  0x143465e90
|     |||   0x1434470aa      mov   rbx, rax
|    ,====< 0x1434470ad      jmp   0x1434470b2
|    |`---> 0x1434470af      mov   rbx, rdi
|    | ||   ; CODE XREF from fcn.143446fb0 @ 0x1434470ad
|    `----> 0x1434470b2      mov   esi, 0x01
|      ||   0x1434470b7      mov   qword [var_b8h], rbx
|      ||   0x1434470bb      mov   r9, qword [r12+0x18]
|      ||   0x1434470c0      mov   r10, qword [r12+0x20]
|      ``-> 0x1434470c5      cmp   r10, 0x10                           ; 16
|       ,=< 0x1434470c9      jb    0x1434470ce
|       |   0x1434470cb      mov   r14, qword [r14]
|       `-> 0x1434470ce      cmp   r9, 0x03                            ; 3
|       ,=< 0x1434470d2      jnz   0x14344713e
|       |   0x1434470d4      mov   rcx, qword [0x1489f9b48]            ; [0x1489f9b48:8]=0x1489f9b58 "sys" ; "X\x9b\x9fH\U00000001"
|       |   0x1434470db      movzx eax, byte [r14]
|       |   0x1434470df      cmp   al, byte [rcx]
|      ,==< 0x1434470e1      jnz   0x1434470fb
|      ||   0x1434470e3      movzx eax, byte [r14+0x01]
|      ||   0x1434470e8      cmp   al, byte [rcx+0x01]
|     ,===< 0x1434470eb      jnz   0x1434470fb
|     |||   0x1434470ed      movzx eax, byte [r14+0x02]
|     |||   0x1434470f2      cmp   al, byte [rcx+0x02]
|    ,====< 0x1434470f5      jnz   0x1434470fb
|    ||||   0x1434470f7      mov   eax, edi
|   ,=====< 0x1434470f9      jmp   0x143447100
|   |```--> 0x1434470fb      sbb   eax, eax
|   |   |   0x1434470fd      or    eax, 0x01
|   |   |   ; CODE XREF from fcn.143446fb0 @ 0x1434470f9
|   `-----> 0x143447100      test  eax, eax
|      ,==< 0x143447102      jnz   0x14344713e
|      ||   0x143447104      lea   ecx, qword [rax+0x38]
|      ||   0x143447107      call  0x14385bce0
|      ||   0x14344710c      mov   qword [var_a8h], rax
|      ||   0x143447110      test  rax, rax
|     ,===< 0x143447113      jz    0x14344711f
|     |||   0x143447115      mov   rcx, rax
|     |||   0x143447118      call  0x14344ba20
|    ,====< 0x14344711d      jmp   0x143447122
|    |`---> 0x14344711f      mov   rax, rdi
|    | ||   ; CODE XREF from fcn.143446fb0 @ 0x14344711d
|    `----> 0x143447122      or    esi, 0x02
|      ||   0x143447125      mov   rcx, rbx
|      ||   0x143447128      mov   rbx, rax
|      ||   0x14344712b      mov   qword [var_b8h], rax
|      ||   0x14344712f      test  rcx, rcx
|     ,===< 0x143447132      jz    0x14344713e
|     |||   0x143447134      mov   rax, qword [rcx]
|     |||   0x143447137      mov   edx, 0x01
|     |||   0x14344713c      call  qword [rax]
|     ```-> 0x14344713e      test  rbx, rbx
|       ,=< 0x143447141      jnz   0x1434471ac
|       |   0x143447143      mov   qword [var_a8h], rdi
|       |   0x143447147      or    esi, 0x04
|       |   0x14344714a      mov   dword [var_78h], esi
|       |   0x14344714d      cmp   byte [0x14d4a6cc8], bl              ; [0x14d4a6cc8:1]=0
|      ,==< 0x143447153      jz    0x143447185
|      ||   0x143447155      lea   ecx, qword [rbx+0x38]
|      ||   0x143447158      call  0x14385bce0
|      ||   0x14344715d      mov   qword [var_c8h], rax
|      ||   0x143447161      test  rax, rax
|     ,===< 0x143447164      jz    0x143447173
|     |||   0x143447166      mov   rcx, rax
|     |||   0x143447169      call  0x143465e90
|     |||   0x14344716e      mov   rbx, rax
|    ,====< 0x143447171      jmp   0x143447176
|    |`---> 0x143447173      mov   rbx, rdi
|    | ||   ; CODE XREF from fcn.143446fb0 @ 0x143447171
|    `----> 0x143447176      or    esi, 0x08
|      ||   0x143447179      mov   dword [var_78h], esi
|      ||   0x14344717c      mov   qword [var_a8h], rbx
|      ||   0x143447180      test  rbx, rbx
|     ,===< 0x143447183      jnz   0x1434471a8
|     |`--> 0x143447185      mov   ecx, 0x38                           ; '8' ; 56
|     | |   0x14344718a      call  0x14385bce0
|     | |   0x14344718f      mov   qword [var_c8h], rax
|     | |   0x143447193      test  rax, rax
|     |,==< 0x143447196      jz    0x1434471a5
|     |||   0x143447198      mov   rcx, rax
|     |||   0x14344719b      call  0x14344ba20
|     |||   0x1434471a0      mov   rbx, rax
|    ,====< 0x1434471a3      jmp   0x1434471a8
|    ||`--> 0x1434471a5      mov   rbx, rdi
|    || |   ; CODE XREF from fcn.143446fb0 @ 0x1434471a3
|    ``---> 0x1434471a8      mov   qword [var_b8h], rbx
|       `-> 0x1434471ac      mov   ecx, 0x18                           ; 24
|           0x1434471b1      call  0x14385bce0
|           0x1434471b6      mov   qword [var_c8h], rax
|           0x1434471ba      test  rax, rax
|       ,=< 0x1434471bd      jz    0x1434471db
|       |   0x1434471bf      mov   rcx, rbx
|       |   0x1434471c2      mov   rbx, rdi
|       |   0x1434471c5      mov   qword [var_b8h], rbx
|       |   0x1434471c9      mov   qword [var_78h], rcx
|       |   0x1434471cd      lea   rdx, qword [var_78h]
|       |   0x1434471d1      mov   rcx, rax
|       |   0x1434471d4      call  0x14344ae50
|      ,==< 0x1434471d9      jmp   0x1434471de
|      |`-> 0x1434471db      mov   rax, rdi
|      |    ; CODE XREF from fcn.143446fb0 @ 0x1434471d9
|      `--> 0x1434471de      mov   rcx, qword [r15+0x28]
|           0x1434471e2      mov   qword [r15+0x28], rax
|           0x1434471e6      test  rcx, rcx
|       ,=< 0x1434471e9      jz    0x1434471f5
|       |   0x1434471eb      mov   rax, qword [rcx]
|       |   0x1434471ee      mov   edx, 0x01
|       |   0x1434471f3      call  qword [rax]
|       `-> 0x1434471f5      mov   rax, qword [0x14c3d0198]            ; [0x14c3d0198:8]=0x406
|           0x1434471fc      cmp   al, 0x06                            ; 6
|       ,=< 0x1434471fe      jb    0x1434472d8
|       |   0x143447204      shr   rax, 0x08
|       |   0x143447208      cmp   al, 0x04                            ; 4
|      ,==< 0x14344720a      jb    0x1434472d8
|      ||   0x143447210      mov   rcx, qword [r15+0x28]
|      ||   0x143447214      mov   rax, qword [rcx]
|      ||   0x143447217      lea   rdx, qword [var_68h]
|      ||   0x14344721b      call  qword [rax+0x30]                    ; 48
|      ||   0x14344721e      nop
|      ||   0x14344721f      lea   rcx, qword [0x1489f9b60]            ; "[DFLog::RbxTransportIoLibContext] RbxTransport I/O backend chosen: {}"
|      ||   0x143447226      mov   qword [var_a8h], rcx
|      ||   0x14344722a      mov   qword [var_a0h], 0x45               ; 'E' ; 69
|      ||   0x143447232      mov   rcx, rax
|      ||   0x143447235      cmp   qword [rax+0x18], 0x10
|     ,===< 0x14344723a      jb    0x14344723f
|     |||   0x14344723c      mov   rcx, qword [rax]
|     `---> 0x14344723f      mov   rax, qword [rax+0x10]
|      ||   0x143447243      mov   qword [var_c8h], rcx
|      ||   0x143447247      mov   qword [var_c0h], rax
|      ||   0x14344724b      movaps xmm0, xmmword [var_c8h]
|      ||   0x14344724f      movdqa xmmword [var_78h], xmm0
|      ||   0x143447254      mov   qword [var_c8h], 0x0d               ; 0xd ; 13
|      ||   0x14344725c      lea   rax, qword [var_78h]
|      ||   0x143447260      mov   qword [var_c0h], rax
|      ||   0x143447264      movaps xmm0, xmmword [var_c8h]
|      ||   0x143447268      movdqa xmmword [var_c8h], xmm0
|      ||   0x14344726d      movaps xmm1, xmmword [var_a8h]
|      ||   0x143447271      movdqa xmmword [var_a8h], xmm1
|      ||   0x143447276      movups xmm0, xmmword [0x14c3d0198]        ; [0x14c3d0198:16]=-1
|      ||   0x14344727d      movaps xmmword [var_88h], xmm0
|      ||   0x143447281      mov   byte [var_d8h], 0x01
|      ||   0x143447286      lea   r9, qword [var_c8h]
|      ||   0x14344728a      lea   r8, qword [var_a8h]
|      ||   0x14344728e      mov   dl, 0x04
|      ||   0x143447290      lea   rcx, qword [var_88h]
|      ||   0x143447294      call  0x1438602b0
|      ||   0x143447299      nop
|      ||   0x14344729a      mov   rdx, qword [var_50h]
|      ||   0x14344729e      cmp   rdx, 0x10                           ; 16
|     ,===< 0x1434472a2      jb    0x1434472d8
|     |||   0x1434472a4      inc   rdx
|     |||   0x1434472a7      mov   rcx, qword [var_68h]
|     |||   0x1434472ab      mov   rax, rcx
|     |||   0x1434472ae      cmp   rdx, 0x1000
|    ,====< 0x1434472b5      jb    0x1434472d3
|    ||||   0x1434472b7      add   rdx, 0x27                           ; 39
|    ||||   0x1434472bb      mov   rcx, qword [rcx-0x08]
|    ||||   0x1434472bf      sub   rax, rcx
|    ||||   0x1434472c2      add   rax, 0xfffffffffffffff8
|    ||||   0x1434472c6      cmp   rax, 0x1f                           ; 31
|   ,=====< 0x1434472ca      jbe   0x1434472d3
|   |||||   0x1434472cc      call  qword [sym.imp.api_ms_win_crt_runtime_l1_1_0.dll__invalid_parameter_noinfo_noreturn] ; [0x148442550:8]=0xc35a02e ; ".\xa05\f"
|   |||||   0x1434472d2      int3
|   ``----> 0x1434472d3      call  fcn.14385bd60
|     ```-> 0x1434472d8      mov   dword [var_78h], edi
|           0x1434472db      lea   rsi, qword [var_78h]
|           0x1434472df      lea   rax, qword [r12+0x04]
|           0x1434472e4      cmp   dword [rax], 0x00
|           0x1434472e7      cmovnle rsi, rax
|           0x1434472eb      mov   ecx, 0x20                           ; 32
|           0x1434472f0      call  0x14385bce0
|           0x1434472f5      mov   qword [var_c8h], rax
|           0x1434472f9      test  rax, rax
|       ,=< 0x1434472fc      jz    0x14344730b
|       |   0x1434472fe      mov   edx, dword [rsi]
|       |   0x143447300      mov   rcx, rax
|       |   0x143447303      call  0x14344c0d0
|       |   0x143447308      mov   rdi, rax
|       `-> 0x14344730b      lea   rax, qword [var_c8h]
|           0x14344730f      cmp   r13, rax
|       ,=< 0x143447312      jz    0x14344732e
|       |   0x143447314      mov   rsi, qword [r13]
|       |   0x143447318      mov   qword [r13], rdi
|       |   0x14344731c      test  rsi, rsi
|      ,==< 0x14344731f      jz    0x143447344
|      ||   0x143447321      mov   rcx, rsi
|      ||   0x143447324      call  0x14344c230
|      ||   0x143447329      mov   rcx, rsi
|     ,===< 0x14344732c      jmp   0x14344733e
|     ||`-> 0x14344732e      test  rdi, rdi
|     ||,=< 0x143447331      jz    0x143447344
|     |||   0x143447333      mov   rcx, rdi
|     |||   0x143447336      call  0x14344c230
|     |||   0x14344733b      mov   rcx, rdi
|     |||   ; CODE XREF from fcn.143446fb0 @ 0x14344732c
|     `---> 0x14344733e      call  fcn.14385bd60
|      ||   0x143447343      nop
|      ``-> 0x143447344      test  rbx, rbx
|       ,=< 0x143447347      jz    0x14344735b
|       |   0x143447349      mov   rcx, qword [rbx]
|       |   0x14344734c      mov   r8, qword [rcx]
|       |   0x14344734f      mov   edx, 0x01
|       |   0x143447354      mov   rcx, rbx
|       |   0x143447357      call  r8
|       |   0x14344735a      nop
|       `-> 0x14344735b      mov   rax, r15
|           0x14344735e      mov   rcx, qword [var_48h]
|           0x143447362      xor   rcx, rsp
|           0x143447365      call  fcn.14730fca0
|           0x14344736a      mov   rbx, qword [var_18h]
|           0x143447372      add   rsp, 0xc0
|           0x143447379      pop   r15
|           0x14344737b      pop   r14
|           0x14344737d      pop   r13
|           0x14344737f      pop   r12
|           0x143447381      pop   rdi
|           0x143447382      pop   rsi
|           0x143447383      pop   rbp
\           0x143447384      ret
