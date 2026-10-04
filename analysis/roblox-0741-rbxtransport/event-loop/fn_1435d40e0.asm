/ fcn.1435d40e0(int64_t arg1);
|           ; arg int64_t arg1 @ rcx
|           ; var int64_t var_e8h @ stack - 0xe8
|           ; var int64_t var_d8h @ stack - 0xd8
|           ; var int64_t var_d0h @ stack - 0xd0
|           ; var int64_t var_c8h @ stack - 0xc8
|           ; var int64_t var_c0h @ stack - 0xc0
|           ; var int64_t var_b8h @ stack - 0xb8
|           ; var int64_t var_b0h @ stack - 0xb0
|           ; var int64_t var_a8h @ stack - 0xa8
|           ; var int64_t var_98h @ stack - 0x98
|           ; var int64_t var_90h @ stack - 0x90
|           ; var int64_t var_88h @ stack - 0x88
|           ; var int64_t var_78h @ stack - 0x78
|           ; var int64_t var_68h @ stack - 0x68
|           ; var int64_t var_60h @ stack - 0x60
|           ; var int64_t var_48h @ stack - 0x48
|           ; var int64_t var_30h @ stack - 0x30
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_10h @ stack + 0x10
|           ; var int64_t var_18h @ stack + 0x18
|           0x1435d40e0      mov   qword [var_10h], rbx
|           0x1435d40e5      mov   qword [var_18h], rsi
|           0x1435d40ea      push  rbp
|           0x1435d40eb      push  rdi
|           0x1435d40ec      push  r12
|           0x1435d40ee      push  r14
|           0x1435d40f0      push  r15
|           0x1435d40f2      lea   rbp, qword [var_60h + 0x1]
|           0x1435d40f7      sub   rsp, 0xe0
|           0x1435d40fe      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x1435d4105      xor   rax, rsp
|           0x1435d4108      mov   qword [var_30h], rax
|           0x1435d410c      mov   rbx, rcx                            ; arg1
|           0x1435d410f      mov   qword [var_b8h], rcx                ; arg1
|           0x1435d4113      lea   rax, qword [0x148aa7348]
|           0x1435d411a      mov   qword [rcx], rax                    ; arg1
|           0x1435d411d      movzx eax, byte [0x14d735648]             ; [0x14d735648:1]=0
|           0x1435d4124      mov   byte [rcx+0x08], al                 ; arg1
|           0x1435d4127      mov   qword [var_b0h], 0x01
|           0x1435d412f      xor   r12d, r12d
|           0x1435d4132      mov   qword [var_a8h], r12
|           0x1435d4136      movdqa xmm0, xmmword [0x1484ae800]
|           0x1435d413e      movdqu xmmword [var_98h], xmm0
|           0x1435d4143      mov   byte [var_a8h], r12b
|           0x1435d4147      lea   rdx, qword [var_b0h]
|           0x1435d414b      add   rcx, 0x10                           ; 16 ; arg1
|           0x1435d414f      call  fcn.143446fb0
|           0x1435d4154      nop
|           0x1435d4155      mov   rdx, qword [var_90h]
|           0x1435d4159      cmp   rdx, 0x10                           ; 16
|       ,=< 0x1435d415d      jb    0x1435d4193
|       |   0x1435d415f      inc   rdx
|       |   0x1435d4162      mov   rcx, qword [var_a8h]
|       |   0x1435d4166      mov   rax, rcx
|       |   0x1435d4169      cmp   rdx, 0x1000
|      ,==< 0x1435d4170      jb    0x1435d418e
|      ||   0x1435d4172      add   rdx, 0x27                           ; 39
|      ||   0x1435d4176      mov   rcx, qword [rcx-0x08]
|      ||   0x1435d417a      sub   rax, rcx
|      ||   0x1435d417d      add   rax, 0xfffffffffffffff8
|      ||   0x1435d4181      cmp   rax, 0x1f                           ; 31
|     ,===< 0x1435d4185      jbe   0x1435d418e
|     |||   0x1435d4187      call  qword [sym.imp.api_ms_win_crt_runtime_l1_1_0.dll__invalid_parameter_noinfo_noreturn] ; [0x148442550:8]=0xc35a02e ; ".\xa05\f"
|     |||   0x1435d418d      int3
|     ``--> 0x1435d418e      call  fcn.14385bd60
|       `-> 0x1435d4193      mov   byte [rbx+0x88], 0x00
|           0x1435d419a      lea   rcx, qword [rbx+0x90]
|           0x1435d41a1      lea   rdx, qword [0x148aa7368]            ; "RtcIoRna::stateMx"
|           0x1435d41a8      call  fcn.1427c81e0
|           0x1435d41ad      nop
|           0x1435d41ae      mov   qword [rbx+0x98], r12
|           0x1435d41b5      mov   qword [rbx+0xa0], r12
|           0x1435d41bc      lea   rdi, qword [rbx+0xa8]
|           0x1435d41c3      mov   byte [rdi+0x68], 0x00
|           0x1435d41c7      lea   rsi, qword [rbx+0x118]
|           0x1435d41ce      mov   byte [rsi+0x68], 0x00
|           0x1435d41d2      lea   r15, qword [rbx+0x188]
|           0x1435d41d9      mov   byte [r15+0x80], 0x00
|           0x1435d41e1      mov   byte [rbx+0x290], 0x00
|           0x1435d41e8      mov   byte [rbx+0x318], 0x00
|           0x1435d41ef      lea   rdx, qword [var_68h]
|           0x1435d41f3      lea   rcx, qword [rbx+0x10]
|           0x1435d41f7      call  fcn.143447b40
|           0x1435d41fc      nop
|           0x1435d41fd      lea   rcx, qword [var_68h]
|           0x1435d4201      call  fcn.14277c520
|           0x1435d4206      test  al, al
|       ,=< 0x1435d4208      jnz   0x1435d4331
|       |   0x1435d420e      mov   rax, qword [0x14c3d5930]            ; [0x14c3d5930:8]=0x406
|       |   0x1435d4215      cmp   al, 0x06                            ; 6
|      ,==< 0x1435d4217      jb    0x1435d42f1
|      ||   0x1435d421d      shr   rax, 0x08
|      ||   0x1435d4221      cmp   al, 0x01                            ; 1
|     ,===< 0x1435d4223      jb    0x1435d42f1
|     |||   0x1435d4229      lea   rdx, qword [var_b0h]
|     |||   0x1435d422d      lea   rcx, qword [var_68h]
|     |||   0x1435d4231      call  fcn.143446900
|     |||   0x1435d4236      nop
|     |||   0x1435d4237      lea   rcx, qword [0x148aa7380]            ; "[DFLog::RbxTransportRnaExp] Failed to start IO event loop thread(s): {}"
|     |||   0x1435d423e      mov   qword [var_c8h], rcx
|     |||   0x1435d4242      mov   qword [var_c0h], 0x47               ; 'G' ; 71
|     |||   0x1435d424a      mov   rcx, rax
|     |||   0x1435d424d      cmp   qword [rax+0x18], 0x10
|    ,====< 0x1435d4252      jb    0x1435d4257
|    ||||   0x1435d4254      mov   rcx, qword [rax]
|    `----> 0x1435d4257      mov   rax, qword [rax+0x10]
|     |||   0x1435d425b      mov   qword [var_d8h], rcx
|     |||   0x1435d425f      mov   qword [var_d0h], rax
|     |||   0x1435d4263      movaps xmm0, xmmword [var_d8h]
|     |||   0x1435d4267      movdqa xmmword [var_78h], xmm0
|     |||   0x1435d426c      mov   qword [var_d8h], 0x0d               ; 0xd ; 13
|     |||   0x1435d4274      lea   rax, qword [var_78h]
|     |||   0x1435d4278      mov   qword [var_d0h], rax
|     |||   0x1435d427c      movaps xmm0, xmmword [var_d8h]
|     |||   0x1435d4280      movdqa xmmword [var_d8h], xmm0
|     |||   0x1435d4285      movaps xmm1, xmmword [var_c8h]
|     |||   0x1435d4289      movdqa xmmword [var_c8h], xmm1
|     |||   0x1435d428e      movups xmm0, xmmword [0x14c3d5930]        ; [0x14c3d5930:16]=-1
|     |||   0x1435d4295      movaps xmmword [var_88h], xmm0
|     |||   0x1435d4299      mov   byte [var_e8h], 0x01
|     |||   0x1435d429e      lea   r9, qword [var_d8h]
|     |||   0x1435d42a2      lea   r8, qword [var_c8h]
|     |||   0x1435d42a6      mov   dl, 0x01
|     |||   0x1435d42a8      lea   rcx, qword [var_88h]
|     |||   0x1435d42ac      call  fcn.1438602b0
|     |||   0x1435d42b1      nop
|     |||   0x1435d42b2      mov   rdx, qword [var_98h]
|     |||   0x1435d42b6      cmp   rdx, 0x10                           ; 16
|    ,====< 0x1435d42ba      jb    0x1435d42f1
|    ||||   0x1435d42bc      inc   rdx
|    ||||   0x1435d42bf      mov   rcx, qword [var_b0h]
|    ||||   0x1435d42c3      mov   rax, rcx
|    ||||   0x1435d42c6      cmp   rdx, 0x1000
|   ,=====< 0x1435d42cd      jb    0x1435d42eb
|   |||||   0x1435d42cf      add   rdx, 0x27                           ; 39
|   |||||   0x1435d42d3      mov   rcx, qword [rcx-0x08]
|   |||||   0x1435d42d7      sub   rax, rcx
|   |||||   0x1435d42da      add   rax, 0xfffffffffffffff8
|   |||||   0x1435d42de      cmp   rax, 0x1f                           ; 31
|  ,======< 0x1435d42e2      jbe   0x1435d42eb
|  ||||||   0x1435d42e4      call  qword [sym.imp.api_ms_win_crt_runtime_l1_1_0.dll__invalid_parameter_noinfo_noreturn] ; [0x148442550:8]=0xc35a02e ; ".\xa05\f"
|  ||||||   0x1435d42ea      int3
|  ``-----> 0x1435d42eb      call  fcn.14385bd60
|    ||||   0x1435d42f0      nop
|    ```--> 0x1435d42f1      mov   rdx, qword [var_48h]
|       |   0x1435d42f5      cmp   rdx, 0x10                           ; 16
|      ,==< 0x1435d42f9      jb    0x1435d4576
|      ||   0x1435d42ff      inc   rdx
|      ||   0x1435d4302      mov   rcx, qword [var_60h]
|      ||   0x1435d4306      mov   rax, rcx
|      ||   0x1435d4309      cmp   rdx, 0x1000
|     ,===< 0x1435d4310      jb    0x1435d4327
|     |||   0x1435d4312      add   rdx, 0x27                           ; 39
|     |||   0x1435d4316      mov   rcx, qword [rcx-0x08]
|     |||   0x1435d431a      sub   rax, rcx
|     |||   0x1435d431d      add   rax, 0xfffffffffffffff8
|     |||   0x1435d4321      cmp   rax, 0x1f                           ; 31
|    ,====< 0x1435d4325      jnbe  0x1435d4363
|    |`---> 0x1435d4327      call  fcn.14385bd60
|    |,===< 0x1435d432c      jmp   0x1435d4576
|    |||`-> 0x1435d4331      mov   rdx, qword [var_48h]
|    |||    0x1435d4335      cmp   rdx, 0x10                           ; 16
|    |||,=< 0x1435d4339      jb    0x1435d436f
|    ||||   0x1435d433b      inc   rdx
|    ||||   0x1435d433e      mov   rcx, qword [var_60h]
|    ||||   0x1435d4342      mov   rax, rcx
|    ||||   0x1435d4345      cmp   rdx, 0x1000
|   ,=====< 0x1435d434c      jb    0x1435d436a
|   |||||   0x1435d434e      add   rdx, 0x27                           ; 39
|   |||||   0x1435d4352      mov   rcx, qword [rcx-0x08]
|   |||||   0x1435d4356      sub   rax, rcx
|   |||||   0x1435d4359      add   rax, 0xfffffffffffffff8
|   |||||   0x1435d435d      cmp   rax, 0x1f                           ; 31
|  ,======< 0x1435d4361      jbe   0x1435d436a
|  ||`----> 0x1435d4363      call  qword [sym.imp.api_ms_win_crt_runtime_l1_1_0.dll__invalid_parameter_noinfo_noreturn] ; [0x148442550:8]=0xc35a02e ; ".\xa05\f"
|  || |||   0x1435d4369      int3
|  ``-----> 0x1435d436a      call  fcn.14385bd60
|     ||`-> 0x1435d436f      lea   rcx, qword [rbx+0x10]
|     ||    0x1435d4373      cmp   byte [rbx+0x08], 0x00
|     ||,=< 0x1435d4377      jz    0x1435d4382
|     |||   0x1435d4379      xor   edx, edx
|     |||   0x1435d437b      call  fcn.1434479f0
|    ,====< 0x1435d4380      jmp   0x1435d4387
|    |||`-> 0x1435d4382      call  fcn.1434479b0
|    |||    ; CODE XREF from fcn.1435d40e0 @ 0x1435d4380
|    `----> 0x1435d4387      mov   r14, rax
|     ||    0x1435d438a      cmp   byte [0x14d7356a8], 0x00            ; [0x14d7356a8:1]=0
|     ||,=< 0x1435d4391      jz    0x1435d43cf
|     |||   0x1435d4393      cmp   byte [r15+0x80], 0x00
|    ,====< 0x1435d439b      jz    0x1435d43af
|    ||||   0x1435d439d      xor   edx, edx
|    ||||   0x1435d439f      mov   rcx, r15
|    ||||   0x1435d43a2      call  fcn.1435d6bc0
|    ||||   0x1435d43a7      mov   byte [r15+0x80], 0x00
|    `----> 0x1435d43af      mov   qword [var_98h], r12
|     |||   0x1435d43b3      lea   r8, qword [var_b0h]
|     |||   0x1435d43b7      mov   rdx, r14
|     |||   0x1435d43ba      mov   rcx, r15
|     |||   0x1435d43bd      call  fcn.1435d3a60
|     |||   0x1435d43c2      mov   byte [r15+0x80], 0x01
|    ,====< 0x1435d43ca      jmp   0x1435d446e
|    |||`-> 0x1435d43cf      cmp   byte [0x14d735620], 0x00            ; [0x14d735620:1]=0
|    |||,=< 0x1435d43d6      jz    0x1435d4424
|    ||||   0x1435d43d8      cmp   byte [rdi+0x68], 0x00
|   ,=====< 0x1435d43dc      jz    0x1435d43ea
|   |||||   0x1435d43de      mov   rcx, rdi
|   |||||   0x1435d43e1      call  fcn.1435d2ba0
|   |||||   0x1435d43e6      mov   byte [rdi+0x68], 0x00
|   `-----> 0x1435d43ea      mov   qword [rdi], r14
|    ||||   0x1435d43ed      lea   rcx, qword [rdi+0x08]
|    ||||   0x1435d43f1      lea   rdx, qword [0x148aa7780]            ; "SharedConnectionRegistryDeprecated"
|    ||||   0x1435d43f8      call  fcn.1427c81e0
|    ||||   0x1435d43fd      nop
|    ||||   0x1435d43fe      mov   qword [rdi+0x1c], r12
|    ||||   0x1435d4402      mov   qword [rdi+0x10], r12
|    ||||   0x1435d4406      mov   dword [rdi+0x18], r12d
|    ||||   0x1435d440a      mov   qword [rdi+0x34], r12
|    ||||   0x1435d440e      mov   qword [rdi+0x28], r12
|    ||||   0x1435d4412      mov   dword [rdi+0x30], r12d
|    ||||   0x1435d4416      mov   dword [rdi+0x40], r12d
|    ||||   0x1435d441a      mov   qword [rdi+0x60], r12
|    ||||   0x1435d441e      mov   byte [rdi+0x68], 0x01
|   ,=====< 0x1435d4422      jmp   0x1435d446e
|   ||||`-> 0x1435d4424      cmp   byte [rsi+0x68], 0x00
|   ||||,=< 0x1435d4428      jz    0x1435d4436
|   |||||   0x1435d442a      mov   rcx, rsi
|   |||||   0x1435d442d      call  fcn.1435d2a80
|   |||||   0x1435d4432      mov   byte [rsi+0x68], 0x00
|   ||||`-> 0x1435d4436      mov   qword [rsi], r14
|   ||||    0x1435d4439      lea   rcx, qword [rsi+0x08]
|   ||||    0x1435d443d      lea   rdx, qword [0x148aa7780]            ; "SharedConnectionRegistryDeprecated"
|   ||||    0x1435d4444      call  fcn.1427c81e0
|   ||||    0x1435d4449      nop
|   ||||    0x1435d444a      mov   qword [rsi+0x1c], r12
|   ||||    0x1435d444e      mov   qword [rsi+0x10], r12
|   ||||    0x1435d4452      mov   dword [rsi+0x18], r12d
|   ||||    0x1435d4456      mov   qword [rsi+0x34], r12
|   ||||    0x1435d445a      mov   qword [rsi+0x28], r12
|   ||||    0x1435d445e      mov   dword [rsi+0x30], r12d
|   ||||    0x1435d4462      mov   dword [rsi+0x40], r12d
|   ||||    0x1435d4466      mov   qword [rsi+0x60], r12
|   ||||    0x1435d446a      mov   byte [rsi+0x68], 0x01
|   ||||    ; CODE XREFS from fcn.1435d40e0 @ 0x1435d43ca, 0x1435d4422
|   ``----> 0x1435d446e      cmp   byte [rbx+0x08], 0x00
|     ||,=< 0x1435d4472      jz    0x1435d44b2
|     |||   0x1435d4474      cmp   byte [rbx+0x318], 0x00
|    ,====< 0x1435d447b      jz    0x1435d4492
|    ||||   0x1435d447d      xor   edx, edx
|    ||||   0x1435d447f      lea   rcx, qword [rbx+0x298]
|    ||||   0x1435d4486      call  fcn.1435d6d40
|    ||||   0x1435d448b      mov   byte [rbx+0x318], 0x00
|    `----> 0x1435d4492      mov   qword [var_98h], r12
|     |||   0x1435d4496      lea   r8, qword [var_b0h]
|     |||   0x1435d449a      mov   rdx, r14
|     |||   0x1435d449d      lea   rcx, qword [rbx+0x298]
|     |||   0x1435d44a4      call  fcn.1435d3a60
|     |||   0x1435d44a9      mov   byte [rbx+0x318], 0x01
|    ,====< 0x1435d44b0      jmp   0x1435d44ee
|    |||`-> 0x1435d44b2      cmp   byte [rbx+0x290], 0x00
|    |||,=< 0x1435d44b9      jz    0x1435d44d0
|    ||||   0x1435d44bb      xor   edx, edx
|    ||||   0x1435d44bd      lea   rcx, qword [rbx+0x210]
|    ||||   0x1435d44c4      call  fcn.1435d6ec0
|    ||||   0x1435d44c9      mov   byte [rbx+0x290], 0x00
|    |||`-> 0x1435d44d0      mov   qword [var_98h], r12
|    |||    0x1435d44d4      lea   r8, qword [var_b0h]
|    |||    0x1435d44d8      mov   rdx, r14
|    |||    0x1435d44db      lea   rcx, qword [rbx+0x210]
|    |||    0x1435d44e2      call  fcn.1435d3a60
|    |||    0x1435d44e7      mov   byte [rbx+0x290], 0x01
|    |||    ; CODE XREF from fcn.1435d40e0 @ 0x1435d44b0
|    `----> 0x1435d44ee      mov   rax, qword [0x14c3d5930]            ; [0x14c3d5930:8]=0x406
|     ||    0x1435d44f5      cmp   al, 0x06                            ; 6
|     ||,=< 0x1435d44f7      jb    0x1435d456f
|     |||   0x1435d44f9      shr   rax, 0x08
|     |||   0x1435d44fd      cmp   al, 0x04                            ; 4
|    ,====< 0x1435d44ff      jb    0x1435d456f
|    ||||   0x1435d4501      lea   rcx, qword [rbx+0x10]
|    ||||   0x1435d4505      call  fcn.143447a00
|    ||||   0x1435d450a      lea   rcx, qword [0x148aa73d0]            ; "[DFLog::RbxTransportRnaExp] Initialized RtcIoRna with {} event loop threads"
|    ||||   0x1435d4511      mov   qword [var_d8h], rcx
|    ||||   0x1435d4515      mov   qword [var_d0h], 0x4b               ; 'K' ; 75
|    ||||   0x1435d451d      mov   qword [var_88h], rax
|    ||||   0x1435d4521      movaps xmm0, xmmword [var_88h]
|    ||||   0x1435d4525      movdqa xmmword [var_88h], xmm0
|    ||||   0x1435d452a      mov   qword [var_c8h], 0x04
|    ||||   0x1435d4532      lea   rax, qword [var_88h]
|    ||||   0x1435d4536      mov   qword [var_c0h], rax
|    ||||   0x1435d453a      movaps xmm0, xmmword [var_c8h]
|    ||||   0x1435d453e      movdqa xmmword [var_78h], xmm0
|    ||||   0x1435d4543      movaps xmm1, xmmword [var_d8h]
|    ||||   0x1435d4547      movdqa xmmword [var_c8h], xmm1
|    ||||   0x1435d454c      movups xmm0, xmmword [0x14c3d5930]        ; [0x14c3d5930:16]=-1
|    ||||   0x1435d4553      movaps xmmword [var_d8h], xmm0
|    ||||   0x1435d4557      mov   byte [var_e8h], 0x01
|    ||||   0x1435d455c      lea   r9, qword [var_78h]
|    ||||   0x1435d4560      lea   r8, qword [var_c8h]
|    ||||   0x1435d4564      mov   dl, 0x04
|    ||||   0x1435d4566      lea   rcx, qword [var_d8h]
|    ||||   0x1435d456a      call  fcn.1438602b0
|    `--`-> 0x1435d456f      mov   byte [rbx+0x88], 0x01
|     ||    ; CODE XREF from fcn.1435d40e0 @ 0x1435d432c
|     ``--> 0x1435d4576      mov   rax, rbx
|           0x1435d4579      mov   rcx, qword [var_30h]
|           0x1435d457d      xor   rcx, rsp
|           0x1435d4580      call  fcn.14730fca0
|           0x1435d4585      lea   r11, qword [var_28h]
|           0x1435d458d      mov   rbx, qword [r11+0x38]
|           0x1435d4591      mov   rsi, qword [r11+0x40]
|           0x1435d4595      mov   rsp, r11
|           0x1435d4598      pop   r15
|           0x1435d459a      pop   r14
|           0x1435d459c      pop   r12
|           0x1435d459e      pop   rdi
|           0x1435d459f      pop   rbp
\           0x1435d45a0      ret
