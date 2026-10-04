/ fcn.140009480(int64_t arg1);
|           ; arg int64_t arg1 @ rcx
|           ; var int64_t var_18h @ stack - 0x18
|           0x140009480      push  rbp
|           0x140009481      push  rsi
|           0x140009482      sub   rsp, 0x28
|           0x140009486      lea   rbp, qword [var_18h]
|           0x14000948b      mov   rsi, rcx                            ; arg1
|           0x14000948e      mov   qword [rcx], 0x00                   ; arg1
|           0x140009495      call  fcn.140002310
|           0x14000949a      mov   rax, rsi
|           0x14000949d      add   rsp, 0x28
|           0x1400094a1      pop   rsi
|           0x1400094a2      pop   rbp
\           0x1400094a3      ret
