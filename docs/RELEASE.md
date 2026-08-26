# Windows 本地发布与完整性校验

## 1. 当前发布定位

Mimic 当前仅作为个人、朋友使用的开源项目维护，不用于商业销售，也不投放应用商店或其他正式分发渠道。当前发布流程不包含商业代码签名、证书购买、证书指纹、时间戳服务或证书生命周期管理；若未来改变分发定位，应重新评审发布威胁模型后再单独引入。

## 2. 发布入口

在仓库根目录执行：

~~~powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-release.ps1
~~~

脚本依次完成前端生产构建、Rust workspace 的 fmt/check/clippy/test、独立 helper release 构建、主程序 release 构建和发布资源复核。release 模式下若 staging helper 缺失，`build.rs` 会失败关闭，禁止生成未固化 helper 哈希的主程序。

## 3. SHA-256 完整性链

发布链按以下顺序工作：

1. 构建 `mimic-elevated-helper.exe`。
2. 将 helper 复制到构建 staging 目录。
3. 构建主程序，并由 `build.rs` 把 helper 的 SHA-256 固化到主程序。
4. 复核 helper 构建副本与打包副本的 SHA-256 完全一致。
5. 复核驱动安装器 SHA-256 与代码内固化值一致，并拒绝缺失、空文件或重解析点资源。

可单独运行以下命令复核发布目录：

~~~powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-release.ps1
~~~

SHA-256 固化校验用于防止高权限操作执行被替换的 helper 或安装器，但不代表第三方发行者身份认证。对于当前个人、朋友使用和开源分发定位，这是已确认的范围选择。

## 4. 安全失败策略

- helper 缺失、哈希不匹配或位于重解析点：主程序不请求 UAC。
- 协议版本、参数、动作、PID 或 nonce 非法：helper 退出 64。
- 调用者、一次性请求或文件布局校验失败：helper 退出 64。
- 固定安装器资源完整性校验失败：helper 退出 65。
- 提权状态或固定维护动作失败：helper 退出 70。
- helper 不接受任意程序路径、shell 命令或附加参数。

## 5. 发布物布局

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
