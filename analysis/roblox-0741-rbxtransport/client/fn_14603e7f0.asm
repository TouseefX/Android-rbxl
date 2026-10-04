/ fcn.14603e7f0(int64_t arg1, int64_t arg2);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; var int64_t var_138h @ stack - 0x138
|           ; var int64_t var_130h @ stack - 0x130
|           ; var int64_t var_128h @ stack - 0x128
|           ; var int64_t var_118h @ stack - 0x118
|           ; var int64_t var_108h @ stack - 0x108
|           ; var int64_t var_100h @ stack - 0x100
|           ; var int64_t var_c8h @ stack - 0xc8
|           ; var int64_t var_b8h @ stack - 0xb8
|           ; var int64_t var_8h @ stack - 0x8
|           ; var int64_t var_18h @ stack + 0x18
|           ; var int64_t var_20h @ stack + 0x20
|           0x14603e7f0      mov   qword [var_18h], rbx
|           0x14603e7f5      mov   qword [var_20h], rsi
|           0x14603e7fa      push  rdi
|           0x14603e7fb      sub   rsp, 0x150
|           0x14603e802      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x14603e809      xor   rax, rsp
|           0x14603e80c      mov   qword [var_b8h + 0xa0], rax
|           0x14603e814      mov   qword [var_118h], rdx               ; arg2
|           0x14603e819      mov   rdi, rcx                            ; arg1
|           0x14603e81c      mov   rbx, qword [0x14cd9a648]            ; [0x14cd9a648:8]=0
|           0x14603e823      mov   rsi, rdx                            ; arg2
|           0x14603e826      mov   qword [var_108h], rcx               ; arg1
|           0x14603e82b      lea   rcx, qword [var_100h]
|           0x14603e830      call  0x1407490a0
|           0x14603e835      call  0x1427c7e70
|           0x14603e83a      lea   r8, qword [0x14603b3d0]
|           0x14603e841      lea   rdx, qword [var_108h]
|           0x14603e846      lea   rcx, qword [var_b8h]
|           0x14603e84e      call  0x1427c8ae0
|           0x14603e853      mov   dword [var_128h], 0x00
|           0x14603e85b      lea   r8, qword [var_b8h]
|           0x14603e863      mov   qword [var_130h], rbx
|           0x14603e868      lea   rdx, qword [0x148f72d48]            ; "BaseClient::ClientConnectThread"
|           0x14603e86f      mov   r9d, 0x10000
|           0x14603e875      mov   byte [var_138h], 0x01
|           0x14603e87a      lea   rcx, qword [var_118h]
|           0x14603e87f      call  0x1427c7fc0
|           0x14603e884      lea   rcx, qword [rdi+0x78]
|           0x14603e888      mov   rdx, rax
|           0x14603e88b      call  0x1427c8090
|           0x14603e890      lea   rcx, qword [var_118h]
|           0x14603e895      call  0x1427c8080
|           0x14603e89a      lea   rcx, qword [var_b8h]
|           0x14603e8a2      call  0x1427c8b10
|           0x14603e8a7      mov   rcx, qword [var_c8h]
|           0x14603e8af      test  rcx, rcx
|       ,=< 0x14603e8b2      jz    0x14603e8c5
|       |   0x14603e8b4      mov   rax, qword [rcx]
|       |   0x14603e8b7      lea   rdx, qword [var_100h]
|       |   0x14603e8bc      cmp   rcx, rdx
|       |   0x14603e8bf      setnz dl
|       |   0x14603e8c2      call  qword [rax+0x20]                    ; 32
|       `-> 0x14603e8c5      mov   rcx, qword [rsi+0x38]
|           0x14603e8c9      test  rcx, rcx
|       ,=< 0x14603e8cc      jz    0x14603e8e2
|       |   0x14603e8ce      mov   rax, qword [rcx]
|       |   0x14603e8d1      cmp   rcx, rsi
|       |   0x14603e8d4      setnz dl
|       |   0x14603e8d7      call  qword [rax+0x20]                    ; 32
|       |   0x14603e8da      mov   qword [rsi+0x38], 0x00
|       `-> 0x14603e8e2      mov   rcx, qword [var_b8h + 0xa0]
|           0x14603e8ea      xor   rcx, rsp
|           0x14603e8ed      call  0x14730fca0
|           0x14603e8f2      lea   r11, qword [var_8h]
|           0x14603e8fa      mov   rbx, qword [r11+0x20]
|           0x14603e8fe      mov   rsi, qword [r11+0x28]
|           0x14603e902      mov   rsp, r11
|           0x14603e905      pop   rdi
\           0x14603e906      ret
