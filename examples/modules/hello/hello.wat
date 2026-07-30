(module
  (import "hematite" "host_call" (func $host_call
    (param i32 i32 i32 i32 i32) (result i32)))

  ;; Two initial pages fit the 64 KiB input at 4096. The declared maximum is
  ;; Hematite's 32 MiB guest-memory limit (512 WebAssembly pages).
  (memory (export "memory") 2 512)

  (data (i32.const 1024) "{\"message\":\"Hello, Hematite!\"}")
  (data (i32.const 1088) "example.hello invoked")

  (func (export "hematite_alloc") (param $size i32) (result i32)
    local.get $size
    i32.const 65536
    i32.le_u
    if (result i32)
      i32.const 4096
    else
      i32.const 0
    end)

  (func (export "hematite_dispatch") (param i32 i32) (result i64)
    i32.const 1
    i32.const 1088
    i32.const 21
    i32.const 0
    i32.const 0
    call $host_call
    drop

    ;; len=30 in the high 32 bits, ptr=1024 in the low 32 bits.
    i64.const 128849019904))
