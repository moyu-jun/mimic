# Parser fuzzing

`mimic-fuzz-runner` repeatedly mutates valid and malformed INI/WAV seeds and invokes only the parser/validator entry points. It does not open devices, mutate application state, or write production data. Any panic is saved under `fuzz/artifacts/` and returns a failing exit code.

Run from the repository root with:

~~~powershell
.\scripts\run-fuzz.ps1 -Iterations 100000 -Seed 7883953907150050933
~~~

The same seed and iteration count reproduce a run. The scheduled Windows workflow performs a longer run and uploads crash artifacts on failure. This bounded runner is used because the supported MSVC development environment does not include Clang/libFuzzer; the corpus and parser entry points remain reusable if `cargo-fuzz` is introduced later.
