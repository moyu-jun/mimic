# Windows 发布验收

## 1. 自动门禁

Stop 控制路径使用 FakeDriver 进行 200 样本分布测量，测试包含长 Delay 中断、按键释放和 Stop 同步确认。门槛为 P95 ≤ 100 ms、最大值 ≤ 250 ms。执行：

~~~powershell
.\scripts\measure-stop-latency.ps1 -Runs 3
~~~

2026-08-26 当前机器三次结果：

| Run | 样本 | P95 | 最大值 | 结果 |
| --- | ---: | ---: | ---: | --- |
| 1 | 200 | 142 µs | 508 µs | 通过 |
| 2 | 200 | 185 µs | 220 µs | 通过 |
| 3 | 200 | 164 µs | 206 µs | 通过 |

该结果验证 Runtime Actor、控制通道和输入释放账本，不替代真实 Interception 驱动与物理设备测量。

发布资源校验执行：

~~~powershell
.\scripts\verify-release.ps1
.\scripts\verify-release.ps1 -RequireSignature -ExpectedAppThumbprint '<thumbprint>'
~~~

校验项包括非空普通文件/重解析点拒绝、helper 构建副本与打包副本 SHA-256 一致、安装器 SHA-256 固化值，以及可选的 Authenticode 强制校验。启用签名强制时，应用与两个 helper 副本还必须匹配指定证书指纹。

## 2. 当前只读预检结果

已在 Windows 10 build 19042、AMD64、普通权限进程执行：

~~~powershell
.\scripts\windows-acceptance.ps1
~~~

资源、重解析点和哈希门禁通过。当前键盘/鼠标 UpperFilters 未检测到 Interception；应用、helper 和安装器均为 `NotSigned`。因此自动预检完成，但驱动真机和正式签名验收不能在当前环境关闭。

脚本在 `artifacts/` 生成带系统信息、哈希、签名状态和人工复选框的验收记录；该目录不提交 Git。

## 3. 真机矩阵

以下动作具有真实外部影响，只能在专用测试机由验收人员执行并勾选脚本生成的记录：

- 普通启动与按需 UAC、UAC 取消；
- 驱动安装、卸载和重启；
- 键盘/鼠标模拟、透传、同键启停、停止释放和快速重启；
- 坐标拾取确认、取消和超时；
- 麦克风正常、占用、权限拒绝；
- 音频设备缺失、后台预热和用户首播延迟验收；
- helper/installer 篡改拒绝；
- portable 目录移动；
- 正式签名候选包验证。

不得把自动预检、FakeDriver 延迟或未签名本地构建记录为上述真机项已通过。
