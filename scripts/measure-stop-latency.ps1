param(
    [ValidateRange(1, 20)]
    [int]$Runs = 3
)

$ErrorActionPreference = "Stop"
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$tauriRoot = Join-Path $repositoryRoot "src-tauri"

Push-Location $tauriRoot
try {
    for ($run = 1; $run -le $Runs; $run++) {
        Write-Host "Stop latency run $run/$Runs"
        cargo test -p mimic runtime::tests::stop_latency_distribution_meets_budget -- --exact --nocapture --test-threads=1
        if ($LASTEXITCODE -ne 0) {
            throw "stop latency gate failed on run $run"
        }
    }
}
finally {
    Pop-Location
}
