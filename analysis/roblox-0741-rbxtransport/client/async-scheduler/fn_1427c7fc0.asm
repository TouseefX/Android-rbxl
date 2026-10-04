/ fcn.1427c7fc0(int64_t arg1, int64_t arg_28h, int64_t arg_30h, int64_t arg_88h);
|           ; arg int64_t arg1 @ rcx
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_20h @ stack - 0x20
|           ; var int64_t var_18h @ stack - 0x18
|           ; var int64_t var_10h @ stack - 0x10
|           ; arg int64_t arg_28h @ stack + 0x28
|           ; arg int64_t arg_30h @ stack + 0x30
|           ; arg int64_t arg_88h @ stack + 0x88
|           0x1427c7fc0      push  rbx
|           0x1427c7fc2      sub   rsp, 0x40
|           0x1427c7fc6      mov   rbx, rcx                            ; arg1
|           0x1427c7fc9      test  rcx, rcx                            ; arg1
|       ,=< 0x1427c7fcc      jz    0x1427c7ffd
|       |   0x1427c7fce      mov   eax, dword [arg_88h]
|       |   0x1427c7fd5      or    eax, 0x01
|       |   0x1427c7fd8      mov   qword [var_10h], 0x00
|       |   0x1427c7fe1      mov   dword [var_18h], eax
|       |   0x1427c7fe5      mov   rax, qword [arg_30h]
|       |   0x1427c7fea      mov   qword [var_20h], rax
|       |   0x1427c7fef      movzx eax, byte [arg_28h]
|       |   0x1427c7ff4      mov   byte [var_28h], al
|       |   0x1427c7ff8      call  fcn.14000aa60
|       `-> 0x1427c7ffd      mov   rax, rbx
|           0x1427c8000      add   rsp, 0x40
|           0x1427c8004      pop   rbx
\           0x1427c8005      ret
