param(
    [ValidateRange(1, 1000000000)]
    [long]$Iterations = 100000,
    [ValidateRange(1, 5242880)]
    [int]$MaxLength = 262144,
    [long]$Seed = 7883953907150050933
)

$ErrorActionPreference = "Stop"
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$tauriRoot = Join-Path $repositoryRoot "src-tauri"

Push-Location $tauriRoot
try {
    cargo run -p mimic-fuzz-runner -- --iterations $Iterations --max-len $MaxLength --seed $Seed
    if ($LASTEXITCODE -ne 0) {
        throw "parser fuzzing failed"
    }
}
finally {
    Pop-Location
}
