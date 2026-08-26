# Windows 发布与签名

## 1. 发布入口

在仓库根目录执行：

~~~powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .scriptsuild-release.ps1
~~~

脚本依次完成前端生产构建、Rust workspace 的 fmt/check/clippy/test、独立 helper release 构建、主程序 release 构建和打包副本 SHA-256 复核。release 模式下若 staging helper 缺失，`build.rs` 会失败关闭，禁止生成未固化 helper 哈希的主程序。

## 2. Authenticode 签名

正式发布前设置证书指纹：

~~~powershell
$env:MIMIC_SIGNING_CERT_THUMBPRINT = '<certificate-thumbprint>'
powershell -NoProfile -ExecutionPolicy Bypass -File .scriptsuild-release.ps1
~~~

脚本先签名 helper 并验证 `Get-AuthenticodeSignature` 为 `Valid`，再把签名后文件的 SHA-256 固化到主程序，最后签名主程序。驱动安装器必须由其发布流程预先签名；其最终 SHA-256 由 `build.rs` 固化并由主程序和 helper 在执行前复核。

没有证书时脚本允许本地验证，但明确输出未签名警告。此类产物不得作为生产签名验收证据。

## 3. 安全失败策略

- helper 缺失、哈希不匹配或位于重解析点：主程序不请求 UAC。
- 协议版本、参数、动作、PID 或 nonce 非法：helper 退出 64。
- 调用者、一次性请求或文件布局校验失败：helper 退出 64。
- 固定安装器资源完整性校验失败：helper 退出 65。
- 提权状态或固定维护动作失败：helper 退出 70。
- helper 不接受任意程序路径、shell 命令或附加参数。

## 4. 发布物布局

~~~text
Mimic/
  mimic.exe
  interception.dll
  driver/
    mimic-elevated-helper.exe
    install-interception.exe
  data/
~~~

`data/` 只承载运行时可写内容；主程序、DLL、helper 和安装器均不得从 `data/` 加载。
