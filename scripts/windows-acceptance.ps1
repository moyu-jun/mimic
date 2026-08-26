[CmdletBinding()]
param(
    [string]$ReleaseRoot,
    [string]$OutputPath,
    [switch]$RequireSignature
)

$ErrorActionPreference = "Stop"
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if ([string]::IsNullOrWhiteSpace($ReleaseRoot)) {
    $ReleaseRoot = Join-Path $repositoryRoot "src-tauri/target/release"
}
$release = (Resolve-Path -LiteralPath $ReleaseRoot).Path
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $OutputPath = Join-Path $repositoryRoot "artifacts/windows-acceptance-$stamp.md"
}
$output = [IO.Path]::GetFullPath($OutputPath)
New-Item -ItemType Directory -Force -Path ([IO.Path]::GetDirectoryName($output)) | Out-Null

$verifyArguments = @{ ReleaseRoot = $release }
if ($RequireSignature) {
    $verifyArguments.RequireSignature = $true
}
& (Join-Path $PSScriptRoot "verify-release.ps1") @verifyArguments

$os = Get-CimInstance Win32_OperatingSystem
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
$isElevated = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

$resources = @(
    [pscustomobject]@{ Name = "mimic.exe"; Path = Join-Path $release "mimic.exe" },
    [pscustomobject]@{ Name = "helper"; Path = Join-Path $release "driver/mimic-elevated-helper.exe" },
    [pscustomobject]@{ Name = "installer"; Path = Join-Path $release "driver/install-interception.exe" }
)
$resourceRows = foreach ($resource in $resources) {
    $signature = Get-AuthenticodeSignature -LiteralPath $resource.Path
    $hash = (Get-FileHash -LiteralPath $resource.Path -Algorithm SHA256).Hash
    "| $($resource.Name) | $hash | $($signature.Status) |"
}

function Read-UpperFilters {
    param([string]$Class)
    $path = Join-Path (Join-Path (Join-Path (Join-Path "HKLM:" "SYSTEM") "CurrentControlSet") "Control") (Join-Path "Class" $Class)
    try {
        $value = (Get-ItemProperty -LiteralPath $path -Name UpperFilters -ErrorAction Stop).UpperFilters
        return (($value | ForEach-Object { $_.ToString() }) -join ", ")
    }
    catch {
        return "<unavailable>"
    }
}

$keyboardFilters = Read-UpperFilters -Class "{4D36E96B-E325-11CE-BFC1-08002BE10318}"
$mouseFilters = Read-UpperFilters -Class "{4D36E96F-E325-11CE-BFC1-08002BE10318}"
$generated = Get-Date -Format "yyyy-MM-dd HH:mm:ss zzz"
$lines = @(
    "# Mimic Windows 验收记录",
    "",
    "生成时间：$generated",
    "",
    "## 自动预检",
    "",
    "- OS：$($os.Caption) $($os.Version)，build $($os.BuildNumber)",
    "- 架构：$env:PROCESSOR_ARCHITECTURE",
    "- 脚本进程管理员权限：$isElevated",
    "- Release 根目录：$release",
    "- 键盘 UpperFilters：$keyboardFilters",
    "- 鼠标 UpperFilters：$mouseFilters",
    "",
    "| 资源 | SHA-256 | Authenticode |",
    "| --- | --- | --- |"
)
$lines += $resourceRows
$lines += @(
    "",
    "自动预检已通过：资源存在、非空、非重解析点，helper 构建/打包哈希一致，安装器哈希符合代码固化值。",
    "",
    "## 必须人工完成的真机矩阵",
    "",
    "- [ ] 普通启动不弹 UAC，主进程保持普通权限。",
    "- [ ] 驱动已安装时，键盘/鼠标模拟、物理输入透传和同键启停正常。",
    "- [ ] 长延迟运行中停止后不再产生旧输入，所有按下键/按钮均释放。",
    "- [ ] 快速停止并重新启动不会执行旧序列事件。",
    "- [ ] 坐标拾取的确认、蒙版中间取消、30 秒超时均恢复预期状态。",
    "- [ ] 录音设备正常、被占用、权限拒绝时界面与旧 WAV/缓存状态正确。",
    "- [ ] 音频设备存在/缺失时启动不阻塞；预热后首播延迟由用户验收。",
    "- [ ] UAC 取消不会改变驱动状态，普通功能继续可用。",
    "- [ ] 安装、卸载驱动只执行固定 installer；完成后重启提示与实际状态一致。",
    "- [ ] 系统重启只在用户确认后执行；测试环境已做好数据保存。",
    "- [ ] 篡改 helper 或 installer 后操作被拒绝，且不会请求/执行任意路径。",
    "- [ ] 整体移动 portable 目录后，配置、日志、音频和临时目录从新位置派生。",
    "- [ ] 正式候选包的 mimic.exe、helper、installer Authenticode 均为 Valid。",
    "",
    "人工项涉及真实输入、UAC、驱动、麦克风和重启，不能由本只读脚本代替。"
)
Set-Content -LiteralPath $output -Value ($lines -join [Environment]::NewLine) -Encoding utf8
Write-Host "Windows acceptance report created: $output"
