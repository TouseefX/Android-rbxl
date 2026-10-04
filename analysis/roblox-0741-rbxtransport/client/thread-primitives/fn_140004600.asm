/ fcn.140004600(int64_t arg1, int64_t arg2, int64_t arg3, int64_t arg4, int64_t arg_28h, int64_t arg_30h);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; arg int64_t arg3 @ r8
|           ; arg int64_t arg4 @ r9
|           ; var int64_t var_168h @ stack - 0x168
|           ; var int64_t var_160h @ stack - 0x160
|           ; var int64_t var_158h @ stack - 0x158
|           ; var int64_t var_148h @ stack - 0x148
|           ; var int64_t var_140h @ stack - 0x140
|           ; var int64_t var_138h @ stack - 0x138
|           ; var int64_t var_108h @ stack - 0x108
|           ; var int64_t var_b8h @ stack - 0xb8
|           ; var int64_t var_a0h @ stack - 0xa0
|           ; var int64_t var_88h @ stack - 0x88
|           ; var int64_t var_80h @ stack - 0x80
|           ; var int64_t var_78h @ stack - 0x78
|           ; var int64_t var_69h @ stack - 0x69
|           ; var int64_t var_68h @ stack - 0x68
|           ; var int64_t var_58h @ stack - 0x58
|           ; arg int64_t arg_28h @ stack + 0x28
|           ; arg int64_t arg_30h @ stack + 0x30
|           0x140004600      push  rbp
|           0x140004601      push  r15
|           0x140004603      push  r14
|           0x140004605      push  r13
|           0x140004607      push  r12
|           0x140004609      push  rsi
|           0x14000460a      push  rdi
|           0x14000460b      push  rbx
|           0x14000460c      sub   rsp, 0x148
|           0x140004613      lea   rbp, qword [var_108h]
|           0x14000461b      movapd xmmword [var_58h], xmm7
|           0x140004623      movapd xmmword [var_68h], xmm6
|           0x14000462b      mov   r15, r9                             ; arg4
|           0x14000462e      mov   r14, r8                             ; arg3
|           0x140004631      mov   rsi, rdx                            ; arg2
|           0x140004634      mov   rdi, rcx                            ; arg1
|           0x140004637      mov   r12, qword [arg_30h]
|           0x14000463e      call  0x140001b80
|           0x140004643      mov   rbx, qword [rax]
|           0x140004646      mov   qword [var_158h], r14
|           0x14000464a      mov   ax, 0x01
|           0x14000464e      lock  xadd word [r14+0x26], ax
|           0x140004655      test  ax, ax
|           0x140004658      setz  cl
|           0x14000465b      mov   rax, qword [0x14c39fe98]            ; [0x14c39fe98:8]=0x1428bd120
|           0x140004662      test  rax, rax
|           0x140004665      setnz dl
|           0x140004668      and   dl, cl
|           0x14000466a      cmp   dl, 0x01                            ; 1
|       ,=< 0x14000466d      jnz   0x140004678
|       |   0x14000466f      lea   rcx, qword [0x1484aeba9]            ; "257f718-Object,40"
|       |   0x140004676      call  rax
|       `-> 0x140004678      test  r12, r12
|       ,=< 0x14000467b      jz    0x1400046b7
|       |   0x14000467d      lea   rax, qword [rdi+0x2080]
|       |   0x140004684      cmp   r12, rax
|       |   0x140004687      setnb al
|       |   0x14000468a      lea   rcx, qword [rdi+0x1b080]
|       |   0x140004691      cmp   r12, rcx
|       |   0x140004694      setb  cl
|       |   0x140004697      and   cl, al
|       |   0x140004699      cmp   cl, 0x01                            ; 1
|      ,==< 0x14000469c      jz    0x140004764
|      ||   0x1400046a2      mov   rax, 0x1000000000000                ; 281474976710656
|      ||   0x1400046ac      lock  add qword [r12+0x28], rax
|     ,===< 0x1400046b2      jmp   0x14000476a
|     ||`-> 0x1400046b7      mov   rax, qword [r14+0x28]
|     ||    0x1400046bb      test  al, 0x02                            ; 2
|     ||,=< 0x1400046bd      jnz   0x1400046c7
|     |||   0x1400046bf      xor   r12d, r12d
|    ,====< 0x1400046c2      jmp   0x14000476a
|    |||`-> 0x1400046c7      mov   rax, qword [r14+0x28]
|    |||    0x1400046cb      test  al, 0x04                            ; 4
|    |||    0x1400046cd      lea   rcx, qword [0x1484aeda0]
|    |||    0x1400046d4      lea   rax, qword [0x1484aedc0]
|    |||    0x1400046db      cmovz rax, rcx
|    |||    0x1400046df      mov   ecx, dword [rdi+0x2008]
|    |||    0x1400046e5      lea   r8, qword [0x1484aedc0]
|    |||    0x1400046ec      lea   rdx, qword [0x1484aede0]            ; "257f718-Kernel,1314"
|    |||    0x1400046f3      cmovz rdx, r8
|    |||    0x1400046f7      cmp   rax, rdx
|    |||,=< 0x1400046fa      jz    0x14000473e
|    ||||   0x1400046fc      lea   r8, qword [rdi+0x2080]
|    ||||   0x140004703      mov   r9d, 0xc4653601
|    ||||   0x140004709      xor   r12d, r12d
|    ||||   0x14000470c      nop   dword [rax], eax
|   .-----> 0x140004710      movsxd r10, dword [rax]
|   :||||   0x140004713      cmp   r10d, ecx
|  ,======< 0x140004716      jnl   0x140004741
|  |:||||   0x140004718      imul  r10, r10, 0x1900
|  |:||||   0x14000471f      lea   r11, qword [r8+r10*1]
|  |:||||   0x140004723      mov   r10d, dword [r8+r10*1+0x28]
|  |:||||   0x140004728      cmp   r10d, r9d
|  |:||||   0x14000472b      cmovnle r12, r11
|  |:||||   0x14000472f      cmovnle r9d, r10d
|  |:||||   0x140004733      add   rax, 0x04
|  |:||||   0x140004737      cmp   rax, rdx
|  |`=====< 0x14000473a      jnz   0x140004710
|  |,=====< 0x14000473c      jmp   0x140004741
|  |||||`-> 0x14000473e      xor   r12d, r12d
|  |||||    ; CODE XREF from fcn.140004600 @ 0x14000473c
|  ``-----> 0x140004741      test  r12, r12
|    |||    0x140004744      setz  cl
|    |||    0x140004747      mov   rax, qword [0x14c39fe98]            ; [0x14c39fe98:8]=0x1428bd120
|    |||    0x14000474e      test  rax, rax
|    |||    0x140004751      setnz dl
|    |||    0x140004754      and   dl, cl
|    |||    0x140004756      cmp   dl, 0x01                            ; 1
|    |||,=< 0x140004759      jnz   0x140004764
|    ||||   0x14000475b      lea   rcx, qword [0x1484aede0]            ; "257f718-Kernel,1314"
|    ||||   0x140004762      call  rax
|    ||``-> 0x140004764      lock  dec dword [r12+0x28]
|    ||     ; CODE XREFS from fcn.140004600 @ 0x1400046b2, 0x1400046c2
|    ``---> 0x14000476a      test  r12b, 0x1f                          ; 31
|           0x14000476e      setnz cl
|           0x140004771      mov   rax, qword [0x14c39fe98]            ; [0x14c39fe98:8]=0x1428bd120
|           0x140004778      test  rax, rax
|           0x14000477b      setnz dl
|           0x14000477e      and   dl, cl
|           0x140004780      cmp   dl, 0x01                            ; 1
|       ,=< 0x140004783      jnz   0x14000478e
|       |   0x140004785      lea   rcx, qword [0x1484aeb5e]            ; "257f718-Thread,45"
|       |   0x14000478c      call  rax
|       `-> 0x14000478e      movzx r13d, byte [arg_28h]
|           0x140004796      nop   word [rax+rax*1], ax
|       .-> 0x1400047a0      mov   rax, qword [r14+0x28]
|       :   0x1400047a4      mov   ecx, eax
|       :   0x1400047a6      and   ecx, 0x1f                           ; 31
|       :   0x1400047a9      or    rcx, r12
|       :   0x1400047ac      lock  cmpxchg qword [r14+0x28], rcx
|       `=< 0x1400047b2      jnz   0x1400047a0
|           0x1400047b4      mov   rax, qword [r15]
|           0x1400047b7      mov   qword [var_148h], rax
|           0x1400047bb      test  rax, rax
|       ,=< 0x1400047be      jz    0x1400047f1
|       |   0x1400047c0      mov   rdx, qword [r15+0x08]
|       |   0x1400047c4      lea   rcx, qword [var_138h]
|       |   0x1400047c8      call  rax
|       |   0x1400047ca      mov   qword [var_140h], rax
|       |   0x1400047ce      mov   qword [var_b8h], 0x00
|       |   0x1400047d6      cmp   rsi, 0xfffff
|      ,==< 0x1400047dd      jnle  0x140004808
|     .---> 0x1400047df      cmp   rsi, 0x10000
|    ,====< 0x1400047e6      jnz   0x14000481a
|    |:||   0x1400047e8      add   rdi, 0x718                          ; 1816
|   ,=====< 0x1400047ef      jmp   0x14000482a
|   ||:|`-> 0x1400047f1      xor   eax, eax
|   ||:|    0x1400047f3      mov   qword [var_140h], rax
|   ||:|    0x1400047f7      mov   qword [var_b8h], 0x00
|   ||:|    0x1400047ff      cmp   rsi, 0xfffff
|   ||`===< 0x140004806      jle   0x1400047df
|   || `--> 0x140004808      cmp   rsi, 0x100000
|   ||  ,=< 0x14000480f      jnz   0x140004823
|   ||  |   0x140004811      add   rdi, 0x738                          ; 1848
|   || ,==< 0x140004818      jmp   0x14000482a
|   |`----> 0x14000481a      add   rdi, 0x728                          ; 1832
|   | ,===< 0x140004821      jmp   0x14000482a
|   | ||`-> 0x140004823      add   rdi, 0x748                          ; 1864
|   | ||    ; CODE XREFS from fcn.140004600 @ 0x1400047ef, 0x140004818, 0x140004821
|   `-``--> 0x14000482a      movzx r15d, r13b
|           0x14000482e      or    r15, r14
|           0x140004831      lea   r8, qword [var_158h]
|           0x140004835      mov   rcx, rbx
|           0x140004838      mov   rdx, rsi
|           0x14000483b      mov   r9, r15
|           0x14000483e      call  0x1400124f0
|           0x140004843      test  al, al
|       ,=< 0x140004845      jz    0x140004884
|     ..--> 0x140004847      mov   rax, qword [var_148h]
|     ::|   0x14000484b      test  rax, rax
|    ,====< 0x14000484e      jz    0x140004859
|    |::|   0x140004850      mov   rcx, qword [var_140h]
|    |::|   0x140004854      mov   rdx, rcx
|    |::|   0x140004857      call  rax
|    `----> 0x140004859      lea   rcx, qword [var_158h]
|     ::|   0x14000485d      call  0x1400132f0
|     ::|   0x140004862      movaps xmm6, xmmword [var_68h]
|     ::|   0x140004869      movaps xmm7, xmmword [var_58h]
|     ::|   0x140004870      add   rsp, 0x148
|     ::|   0x140004877      pop   rbx
|     ::|   0x140004878      pop   rdi
|     ::|   0x140004879      pop   rsi
|     ::|   0x14000487a      pop   r12
|     ::|   0x14000487c      pop   r13
|     ::|   0x14000487e      pop   r14
|     ::|   0x140004880      pop   r15
|     ::|   0x140004882      pop   rbp
|     ::|   0x140004883      ret
|     ::`-> 0x140004884      lea   r14, qword [var_158h]
|     ::    0x140004888      lea   r13, qword [0x1484aee67]            ; "STM::stack"
|     ::    0x14000488f      lea   r12, qword [var_a0h]
|     ::    0x140004893      movsd xmm6, qword [0x1484ae818]           ; [0x1484ae818:8]=0x43e0000000000000
|     ::    0x14000489b      movsd xmm7, qword [0x1484ae810]           ; [0x1484ae810:8]=0x412e848000000000
|     ::,=< 0x1400048a3      jmp   0x1400048c5
..
|     ::|   ; CODE XREF from fcn.140004600 @ 0x140004a17
| ....----> 0x1400048b0      mov   rcx, rbx
| ::::::|   0x1400048b3      mov   rdx, rsi
| ::::::|   0x1400048b6      mov   r8, r14
| ::::::|   0x1400048b9      mov   r9, r15
| ::::::|   0x1400048bc      call  0x1400124f0
| ::::::|   0x1400048c1      test  al, al
| ::::`===< 0x1400048c3      jnz   0x140004847
| :::: :|   ; CODE XREF from fcn.140004600 @ 0x1400048a3
| :::: :`-> 0x1400048c5      mov   eax, dword [rbx+0x20]
| :::: :    0x1400048c8      and   eax, 0x01
| :::: :    0x1400048cb      lea   rcx, qword [rdi+rax*8]
| :::: :    0x1400048cf      mov   qword [var_80h], rcx
| :::: :    0x1400048d6      mov   rax, qword [rdi+rax*8]
| :::: :    0x1400048da      shr   rax, 0x30
| :::: :    0x1400048de      mov   qword [var_78h], rax
| :::: :    0x1400048e5      mov   rcx, rbx
| :::: :    0x1400048e8      mov   rdx, rsi
| :::: :    0x1400048eb      mov   r8, r14
| :::: :    0x1400048ee      mov   r9, r15
| :::: :    0x1400048f1      call  0x1400124f0
| :::: :    0x1400048f6      test  al, al
| :::: `==< 0x1400048f8      jnz   0x140004847
| ::::      0x1400048fe      mov   rdx, qword [rbx+0x70]
| ::::      0x140004902      mov   rax, qword [var_80h]
| ::::      0x140004909      mov   rax, qword [rax]
| ::::      0x14000490c      shr   rax, 0x30
| ::::      0x140004910      cmp   qword [var_78h], rax
| `=======< 0x140004917      jnz   0x1400048b0
|  :::      0x140004919      and   rdx, 0xffffffffffffffc0
|  :::      0x14000491d      mov   qword [var_a0h], rdx
|  :::      0x140004921      movupd xmm0, xmmword [var_80h]
|  :::      0x140004929      movupd xmmword [var_a0h + 0x8], xmm0
|  :::      0x14000492e      lea   rax, qword [var_69h]
|  :::      0x140004935      mov   qword [var_88h], rax
|  :::      0x14000493c      mov   qword [var_160h], r13
|  :::      0x140004941      mov   dword [var_168h], 0x01
|  :::      0x140004949      mov   rcx, rbx
|  :::      0x14000494c      mov   r8, r12
|  :::      0x14000494f      xor   r9d, r9d
|  :::      0x140004952      call  0x140011f50
|  :::      0x140004957      test  al, al
|  `======< 0x140004959      jnz   0x1400048b0
|   ::      0x14000495f      mov   eax, dword [0x14cb874a8]            ; [0x14cb874a8:4]=0
|   ::      0x140004965      cmp   eax, 0x02                           ; 2
|   ::  ,=< 0x140004968      jz    0x1400049ab
|   ::  |   0x14000496a      xor   eax, eax
|   ::  |   0x14000496c      mov   ecx, 0x01
|   ::  |   0x140004971      lock  cmpxchg dword [0x14cb874a8], ecx
|   :: ,==< 0x140004979      jnz   0x140004a20
|   :: ||   0x14000497f      mov   rcx, r12
|   :: ||   0x140004982      call  qword [sym.imp.KERNEL32.dll_QueryPerformanceFrequency] ; [0x148438af8:8]=0xc35af46 ; "F\xaf5\f" ; BOOL QueryPerformanceFrequency(LARGE_INTEGER *lpFrequency)
|   :: ||   0x140004988      xorps xmm0, xmm0
|   :: ||   0x14000498b      cvtsi2sd xmm0, qword [var_a0h]
|   :: ||   0x140004991      movapd xmm1, xmm7
|   :: ||   0x140004995      divsd xmm1, xmm0
|   :: ||   0x140004999      movsd qword [0x14cb874b0], xmm1           ; [0x14cb874b0:8]=0
|   :: ||   0x1400049a1      mov   dword [0x14cb874a8], 0x02           ; [0x14cb874a8:4]=0
|   ::.-`-> 0x1400049ab      mov   rcx, r12
|   :::|    0x1400049ae      call  qword [sym.imp.KERNEL32.dll_QueryPerformanceCounter] ; [0x148438b00:8]=0xc35af62 ; "b\xaf5\f" ; BOOL QueryPerformanceCounter(LARGE_INTEGER *lpPerformanceCount)
|   :::|    0x1400049b4      xorps xmm0, xmm0
|   :::|    0x1400049b7      cvtsi2sd xmm0, qword [var_a0h]
|   :::|    0x1400049bd      mulsd xmm0, qword [0x14cb874b0]
|   :::|    0x1400049c5      cvttsd2si rax, xmm0
|   :::|    0x1400049ca      mov   rcx, rax
|   :::|    0x1400049cd      subsd xmm0, xmm6
|   :::|    0x1400049d1      cvttsd2si r8, xmm0
|   :::|    0x1400049d6      sar   rcx, 0x3f
|   :::|    0x1400049da      and   r8, rcx
|   :::|    0x1400049dd      or    r8, rax
|   :::|    0x1400049e0      add   r8, 0xf4240
|   :::|    0x1400049e7      lea   rcx, qword [var_80h]
|   :::|    0x1400049ee      mov   rdx, rbx
|   :::|    0x1400049f1      call  0x140010560
|   :::|    0x1400049f6      mov   rdx, qword [0x14c39fe98]            ; [0x14c39fe98:8]=0x1428bd120
|   :::|    0x1400049fd      test  rdx, rdx
|   :::|    0x140004a00      setnz cl
|   :::|    0x140004a03      and   cl, al
|   :::|    0x140004a05      cmp   cl, 0x01                            ; 1
|   `=====< 0x140004a08      jnz   0x1400048b0
|    ::|    0x140004a0e      lea   rcx, qword [0x1484aee72]            ; "257f718-Kernel,1642"
|    ::|    0x140004a15      call  rdx
|    `====< 0x140004a17      jmp   0x1400048b0
..
|     :|    ; CODE XREF from fcn.140004600 @ 0x140004a2b
|     :`.-> 0x140004a20      cmp   eax, 0x02                           ; 2
|     `===< 0x140004a23      jz    0x1400049ab
|       :   0x140004a25      mov   eax, dword [0x14cb874a8]            ; [0x14cb874a8:4]=0
\       `=< 0x140004a2b      jmp   0x140004a20
