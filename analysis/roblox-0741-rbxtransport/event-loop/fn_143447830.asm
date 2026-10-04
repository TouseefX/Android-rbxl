/ fcn.143447830(int64_t arg1, int64_t arg2, int64_t arg_30h, int64_t arg_40h, int64_t arg_48h);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; var int64_t var_68h @ stack - 0x68
|           ; var int64_t var_60h @ stack - 0x60
|           ; var int64_t var_58h @ stack - 0x58
|           ; var int64_t var_50h @ stack - 0x50
|           ; var int64_t var_48h @ stack - 0x48
|           ; var int64_t var_38h @ stack - 0x38
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_18h @ stack + 0x18
|           ; arg int64_t arg_30h @ stack + 0x30
|           ; arg int64_t arg_40h @ stack + 0x40
|           ; arg int64_t arg_48h @ stack + 0x48
|           0x143447830      mov   qword [var_18h], rbx
|           0x143447835      push  rbp
|           0x143447836      push  rsi
|           0x143447837      push  rdi
|           0x143447838      sub   rsp, 0x70
|           0x14344783c      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x143447843      xor   rax, rsp
|           0x143447846      mov   qword [var_28h], rax
|           0x14344784b      mov   rdi, rdx                            ; arg2
|           0x14344784e      mov   rbp, rcx                            ; arg1
|           0x143447851      mov   qword [var_68h], rdx                ; arg2
|           0x143447856      mov   rcx, qword [rcx+0x28]               ; arg1
|           0x14344785a      mov   rax, qword [rcx]                    ; arg1
|           0x14344785d      lea   rdx, qword [var_58h]
|           0x143447862      call  qword [rax+0x08]                    ; 8
|           0x143447865      nop
|           0x143447866      lea   rcx, qword [var_50h]
|           0x14344786b      call  0x14277c520
|           0x143447870      test  al, al
|       ,=< 0x143447872      jnz   0x1434478cb
|       |   0x143447874      xor   esi, esi
|       |   0x143447876      mov   qword [rdi], rsi
|       |   0x143447879      mov   eax, dword [var_50h]
|       |   0x14344787d      mov   dword [rdi+0x08], eax
|       |   0x143447880      movups xmm0, xmmword [var_48h]
|       |   0x143447885      movups xmmword [rdi+0x10], xmm0
|       |   0x143447889      movups xmm1, xmmword [var_38h]
|       |   0x14344788e      movups xmmword [rdi+0x20], xmm1
|       |   0x143447892      movdqa xmm0, xmmword [0x1484ae800]
|       |   0x14344789a      movdqu xmmword [var_38h], xmm0
|       |   0x1434478a0      mov   byte [var_48h], sil
|       |   0x1434478a5      lea   rcx, qword [var_50h]
|       |   0x1434478aa      call  0x14073a8e0
|       |   0x1434478af      mov   rcx, qword [var_58h]
|       |   0x1434478b4      test  rcx, rcx
|      ,==< 0x1434478b7      jz    0x143447985
|      ||   0x1434478bd      mov   r8, qword [rcx]
|      ||   0x1434478c0      lea   edx, qword [rsi+0x01]
|      ||   0x1434478c3      call  qword [r8]
|     ,===< 0x1434478c6      jmp   0x143447985
|     ||`-> 0x1434478cb      mov   ecx, 0x5c0                          ; 1472
|     ||    0x1434478d0      call  0x14385bce0
|     ||    0x1434478d5      mov   qword [var_68h], rax
|     ||    0x1434478da      xor   esi, esi
|     ||    0x1434478dc      test  rax, rax
|     ||,=< 0x1434478df      jz    0x143447902
|     |||   0x1434478e1      mov   rcx, qword [var_58h]
|     |||   0x1434478e6      mov   qword [var_58h], rsi
|     |||   0x1434478eb      mov   qword [var_60h], rcx
|     |||   0x1434478f0      lea   r8, qword [var_60h]
|     |||   0x1434478f5      mov   rdx, rbp
|     |||   0x1434478f8      mov   rcx, rax
|     |||   0x1434478fb      call  0x143445a60
|    ,====< 0x143447900      jmp   0x143447905
|    |||`-> 0x143447902      mov   rax, rsi
|    |||    ; CODE XREF from fcn.143447830 @ 0x143447900
|    `----> 0x143447905      mov   qword [var_68h], rax
|     ||    0x14344790a      mov   rdx, qword [arg_40h]
|     ||    0x14344790e      cmp   rdx, qword [arg_48h]
|     ||,=< 0x143447912      jz    0x143447921
|     |||   0x143447914      mov   rbx, rsi
|     |||   0x143447917      mov   qword [rdx], rax
|     |||   0x14344791a      add   qword [arg_40h], 0x08
|    ,====< 0x14344791f      jmp   0x143447934
|    |||`-> 0x143447921      lea   r8, qword [var_68h]
|    |||    0x143447926      lea   rcx, qword [arg_30h]
|    |||    0x14344792a      call  0x143446c30
|    |||    0x14344792f      mov   rbx, qword [var_68h]
|    |||    ; CODE XREF from fcn.143447830 @ 0x14344791f
|    `----> 0x143447934      test  rbx, rbx
|     ||,=< 0x143447937      jz    0x143447949
|     |||   0x143447939      mov   rcx, rbx
|     |||   0x14344793c      call  0x143445bd0
|     |||   0x143447941      mov   rcx, rbx
|     |||   0x143447944      call  0x14385bd60
|     ||`-> 0x143447949      mov   rax, qword [arg_40h]
|     ||    0x14344794d      mov   rax, qword [rax-0x08]
|     ||    0x143447951      mov   qword [rdi], rax
|     ||    0x143447954      mov   dword [rdi+0x08], esi
|     ||    0x143447957      mov   qword [rdi+0x10], rsi
|     ||    0x14344795b      mov   qword [rdi+0x20], rsi
|     ||    0x14344795f      mov   qword [rdi+0x28], 0x0f              ; [0xf:8]=-1 ; 15
|     ||    0x143447967      lea   rcx, qword [var_50h]
|     ||    0x14344796c      call  0x14073a8e0
|     ||    0x143447971      mov   rcx, qword [var_58h]
|     ||    0x143447976      test  rcx, rcx
|     ||,=< 0x143447979      jz    0x143447985
|     |||   0x14344797b      mov   rax, qword [rcx]
|     |||   0x14344797e      mov   edx, 0x01
|     |||   0x143447983      call  qword [rax]
|     |||   ; CODE XREF from fcn.143447830 @ 0x1434478c6
|     ```-> 0x143447985      mov   rax, rdi
|           0x143447988      mov   rcx, qword [var_28h]
|           0x14344798d      xor   rcx, rsp
|           0x143447990      call  0x14730fca0
|           0x143447995      mov   rbx, qword [var_18h]
|           0x14344799d      add   rsp, 0x70
|           0x1434479a1      pop   rdi
|           0x1434479a2      pop   rsi
|           0x1434479a3      pop   rbp
\           0x1434479a4      ret
