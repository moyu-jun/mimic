[CmdletBinding()]
param(
    [string]$ReleaseRoot,
    [switch]$RequireSignature,
    [string]$ExpectedAppThumbprint = $env:MIMIC_SIGNING_CERT_THUMBPRINT
)

$ErrorActionPreference = "Stop"
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if ([string]::IsNullOrWhiteSpace($ReleaseRoot)) {
    $ReleaseRoot = Join-Path $repositoryRoot "src-tauri/target/release"
}
$ExpectedInstallerSha256 = "E137863A79DA797F08E7A137280FF2A123809044A888FD75CE9C973198915ABE"
$release = (Resolve-Path -LiteralPath $ReleaseRoot).Path

function Assert-RealDirectory {
    param([Parameter(Mandatory = $true)][string]$Path)
    $item = Get-Item -LiteralPath $Path -Force
    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "release directory is missing, not a directory, or a reparse point: $Path"
    }
}

function Assert-RegularFile {
    param([Parameter(Mandatory = $true)][string]$Path)
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or $item.Length -le 0 -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "release resource is empty, not a regular file, or a reparse point: $Path"
    }
    return $item
}

function Normalize-Thumbprint {
    param([AllowNull()][string]$Value)
    if ([string]::IsNullOrWhiteSpace($Value)) {
        return ""
    }
    return $Value.Replace(" ", "").ToUpperInvariant()
}

Assert-RealDirectory -Path $release
$driverDirectory = Join-Path $release "driver"
Assert-RealDirectory -Path $driverDirectory

$paths = [ordered]@{
    Application = Join-Path $release "mimic.exe"
    HelperBuild = Join-Path $release "mimic-elevated-helper.exe"
    HelperPackage = Join-Path $driverDirectory "mimic-elevated-helper.exe"
    Installer = Join-Path $driverDirectory "install-interception.exe"
    InterceptionDll = Join-Path $release "interception.dll"
}

$results = foreach ($entry in $paths.GetEnumerator()) {
    $item = Assert-RegularFile -Path $entry.Value
    $hash = (Get-FileHash -LiteralPath $entry.Value -Algorithm SHA256).Hash
    $signature = if ($entry.Value.EndsWith(".exe", [StringComparison]::OrdinalIgnoreCase)) {
        Get-AuthenticodeSignature -LiteralPath $entry.Value
    } else {
        $null
    }
    [pscustomobject]@{
        Resource = $entry.Key
        Bytes = $item.Length
        Sha256 = $hash
        Signature = if ($null -eq $signature) { "NotChecked" } else { $signature.Status.ToString() }
        SignerThumbprint = if ($null -eq $signature -or $null -eq $signature.SignerCertificate) {
            ""
        } else {
            $signature.SignerCertificate.Thumbprint
        }
    }
}

$byName = @{}
foreach ($result in $results) {
    $byName[$result.Resource] = $result
}
if ($byName.HelperBuild.Sha256 -ne $byName.HelperPackage.Sha256) {
    throw "built helper and packaged helper SHA-256 do not match"
}
if ($byName.Installer.Sha256 -ne $ExpectedInstallerSha256) {
    throw "driver installer SHA-256 does not match the build-time pin"
}

if ($RequireSignature) {
    foreach ($name in @("Application", "HelperBuild", "HelperPackage", "Installer")) {
        if ($byName[$name].Signature -ne "Valid") {
            throw "Authenticode signature is not valid for $name"
        }
    }

    $expected = Normalize-Thumbprint -Value $ExpectedAppThumbprint
    if ([string]::IsNullOrWhiteSpace($expected)) {
        throw "ExpectedAppThumbprint is required when signature enforcement is enabled"
    }
    foreach ($name in @("Application", "HelperBuild", "HelperPackage")) {
        $actual = Normalize-Thumbprint -Value $byName[$name].SignerThumbprint
        if ($actual -ne $expected) {
            throw "signer thumbprint does not match the release certificate for $name"
        }
    }
}

$results | Format-Table -AutoSize
if (-not $RequireSignature -and ($results | Where-Object { $_.Signature -eq "NotSigned" })) {
    Write-Warning "One or more executable resources are unsigned; hash verification passed, production signing did not."
}
Write-Host "Release resource verification passed: $release"
