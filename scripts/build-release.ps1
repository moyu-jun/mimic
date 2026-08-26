param(
    [string]$CertificateThumbprint = $env:MIMIC_SIGNING_CERT_THUMBPRINT,
    [string]$TimestampUrl = "http://timestamp.digicert.com"
)

$ErrorActionPreference = "Stop"
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$tauriRoot = Join-Path $repositoryRoot "src-tauri"
$generatedHelper = Join-Path $repositoryRoot "extra\driver\mimic-elevated-helper.exe"
$helperBuild = Join-Path $tauriRoot "target\release\mimic-elevated-helper.exe"
$applicationBuild = Join-Path $tauriRoot "target\release\mimic.exe"
$packagedHelper = Join-Path $tauriRoot "target\release\driver\mimic-elevated-helper.exe"

function Invoke-CodeSign {
    param([Parameter(Mandatory = $true)][string]$Path)

    if ([string]::IsNullOrWhiteSpace($CertificateThumbprint)) {
        return
    }
    $signTool = Get-Command "signtool.exe" -ErrorAction Stop
    & $signTool.Source sign /sha1 $CertificateThumbprint /fd SHA256 /tr $TimestampUrl /td SHA256 $Path
    if ($LASTEXITCODE -ne 0) {
        throw "signtool failed for $Path"
    }
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne "Valid") {
        throw "Authenticode validation failed for ${Path}: $($signature.Status)"
    }
}

Push-Location $repositoryRoot
try {
    npm run build
    if ($LASTEXITCODE -ne 0) { throw "frontend build failed" }

    Push-Location $tauriRoot
    try {
        cargo fmt --all -- --check
        if ($LASTEXITCODE -ne 0) { throw "cargo fmt failed" }
        cargo check --workspace --all-targets --all-features
        if ($LASTEXITCODE -ne 0) { throw "cargo check failed" }
        cargo clippy --workspace --all-targets --all-features -- -D warnings
        if ($LASTEXITCODE -ne 0) { throw "cargo clippy failed" }
        cargo test --workspace --all-targets
        if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }

        cargo build --release -p mimic-elevated-helper
        if ($LASTEXITCODE -ne 0) { throw "helper release build failed" }
        Invoke-CodeSign -Path $helperBuild

        Copy-Item -LiteralPath $helperBuild -Destination $generatedHelper -Force
        cargo build --release -p mimic
        if ($LASTEXITCODE -ne 0) { throw "application release build failed" }
        Invoke-CodeSign -Path $applicationBuild
    }
    finally {
        Pop-Location
    }

    if (-not (Test-Path -LiteralPath $packagedHelper -PathType Leaf)) {
        throw "packaged helper is missing"
    }
    $sourceHash = (Get-FileHash -LiteralPath $helperBuild -Algorithm SHA256).Hash
    $packagedHash = (Get-FileHash -LiteralPath $packagedHelper -Algorithm SHA256).Hash
    if ($sourceHash -ne $packagedHash) {
        throw "packaged helper hash does not match the embedded release helper"
    }

    Write-Host "Release verified: $applicationBuild"
    Write-Host "Helper SHA-256: $sourceHash"
    if ([string]::IsNullOrWhiteSpace($CertificateThumbprint)) {
        Write-Warning "No signing certificate supplied; binaries are hash-pinned but unsigned."
    }
}
finally {
    Remove-Item -LiteralPath $generatedHelper -Force -ErrorAction SilentlyContinue
    Pop-Location
}
