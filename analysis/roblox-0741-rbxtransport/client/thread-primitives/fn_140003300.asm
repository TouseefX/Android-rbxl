/ fcn.140003300(int64_t arg1, int64_t arg2, int64_t arg3, int64_t arg_28h, int64_t arg_30h);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; arg int64_t arg3 @ r8
|           ; var int64_t var_58h @ stack - 0x58
|           ; var int64_t var_50h @ stack - 0x50
|           ; var int64_t var_48h @ stack - 0x48
|           ; var int64_t var_40h @ stack - 0x40
|           ; var int64_t var_38h @ stack - 0x38
|           ; var int64_t var_30h @ stack - 0x30
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_21h @ stack - 0x21
|           ; arg int64_t arg_28h @ stack + 0x28
|           ; arg int64_t arg_30h @ stack + 0x30
|           0x140003300      push  rbp
|           0x140003301      push  rsi
|           0x140003302      push  rdi
|           0x140003303      push  rbx
|           0x140003304      sub   rsp, 0x58
|           0x140003308      lea   rbp, qword [var_28h]
|           0x14000330d      mov   rdi, rcx                            ; arg1
|           0x140003310      mov   byte [var_21h], r9b
|           0x140003314      mov   qword [var_38h], r8                 ; arg3
|           0x140003318      mov   qword [var_30h], rdx                ; arg2
|           0x14000331c      call  0x140001b80
|           0x140003321      mov   rax, qword [rax]
|           0x140003324      mov   rsi, qword [rax+0x70]
|           0x140003328      and   rsi, 0xffffffffffffffc0
|           0x14000332c      lea   rbx, qword [rsi+0x500]
|           0x140003333      call  0x140001b80
|           0x140003338      mov   rax, qword [rax]
|           0x14000333b      mov   rax, qword [rax+0x68]
|           0x14000333f      mov   rax, qword [rax+0x60]
|           0x140003343      test  rax, rax
|       ,=< 0x140003346      jz    0x140003353
|       |   0x140003348      mov   ecx, dword [rax+0x60]
|       |   0x14000334b      add   rax, 0xc8                           ; 200
|      ,==< 0x140003351      jmp   0x140003360
|      |`-> 0x140003353      mov   ecx, dword [rsi+0x780]
|      |    0x140003359      lea   rax, qword [rsi+0x7e8]
|      |    ; CODE XREF from fcn.140003300 @ 0x140003351
|      `--> 0x140003360      xor   r8d, r8d
|           0x140003363      test  cl, 0x04                            ; 4
|           0x140003366      cmovnz r8, rax
|           0x14000336a      lea   rax, qword [arg_30h]
|           0x14000336e      mov   qword [var_40h], rax
|           0x140003373      lea   rax, qword [arg_28h]
|           0x140003377      mov   qword [var_48h], rax
|           0x14000337c      lea   rax, qword [var_21h]
|           0x140003380      mov   qword [var_50h], rax
|           0x140003385      lea   rax, qword [var_38h]
|           0x140003389      mov   qword [var_58h], rax
|           0x14000338e      lea   r9, qword [var_30h]
|           0x140003392      mov   rcx, rbx
|           0x140003395      mov   rdx, rdi
|           0x140003398      call  0x140003410
|           0x14000339d      mov   rdi, rax
|           0x1400033a0      movzx ebx, byte [rax+0x25]
|           0x1400033a4      cmp   bl, 0x07                            ; 7
|           0x1400033a7      setnb cl
|           0x1400033aa      mov   rax, qword [0x14c39fe98]            ; [0x14c39fe98:8]=0x1428bd120
|           0x1400033b1      test  rax, rax
|           0x1400033b4      setnz dl
|           0x1400033b7      and   dl, cl
|           0x1400033b9      cmp   dl, 0x01                            ; 1
|       ,=< 0x1400033bc      jnz   0x1400033c7
|       |   0x1400033be      lea   rcx, qword [0x1484aed78]            ; "257f718-Kernel,1253"
|       |   0x1400033c5      call  rax
|       `-> 0x1400033c7      call  0x140001b80
|           0x1400033cc      mov   rax, qword [rax]
|           0x1400033cf      mov   rax, qword [rax+0x68]
|           0x1400033d3      mov   rcx, qword [rax+0x60]
|           0x1400033d7      test  rcx, rcx
|       ,=< 0x1400033da      jz    0x1400033e9
|       |   0x1400033dc      mov   r8, rdi
|       |   0x1400033df      add   r8, 0x10                            ; 16
|       |   0x1400033e3      add   rcx, 0x60                           ; 96
|      ,==< 0x1400033e7      jmp   0x1400033fa
|      |`-> 0x1400033e9      add   rsi, 0x780                          ; 1920
|      |    0x1400033f0      lea   r8, qword [0x1484aed8c]
|      |    0x1400033f7      mov   rcx, rsi
|      |    ; CODE XREF from fcn.140003300 @ 0x1400033e7
|      `--> 0x1400033fa      mov   edx, ebx
|           0x1400033fc      mov   r9b, 0x01
|           0x1400033ff      call  0x140011270
|           0x140003404      mov   rax, rdi
|           0x140003407      add   rsp, 0x58
|           0x14000340b      pop   rbx
|           0x14000340c      pop   rdi
|           0x14000340d      pop   rsi
|           0x14000340e      pop   rbp
\           0x14000340f      ret
